use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::display::{AnimFrame, DisplayMode, LedDisplay, WindowClosedError};
use crate::proto::DisplayMode as ProtoDisplayMode;
use crate::shutdown::{Shutdown, Stopped, Work};

pub struct DisplayRequest {
    pub image_data: Vec<u8>,
    pub mime_type: String,
    pub duration: Duration,
    pub display_mode: ProtoDisplayMode,
}

/// Sequential display processing on the main thread, including idle window events.
pub fn run_loop(
    mut display: Box<dyn LedDisplay>,
    rx: Receiver<DisplayRequest>,
    cfg: &Config,
    shutdown: &Shutdown,
) -> anyhow::Result<()> {
    let _guard = shutdown.guard();
    // Load lazily: decoding the eye-catch counts against the first request's budget.
    let mut eyecatch_frames = None;
    let mut eyecatch_loaded = false;
    while !shutdown.is_cancelled() {
        if let Err(e) = display.poll_events() {
            if e.is::<WindowClosedError>() {
                break;
            }
            return Err(e);
        }
        let req = match rx.recv_timeout(Duration::from_millis(16)) {
            Ok(req) => req,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let work = Work {
            shutdown,
            deadline: Instant::now()
                .checked_add(req.duration.min(cfg.worker_timeout))
                .ok_or_else(|| anyhow::anyhow!("request duration is too large"))?,
        };
        if shutdown.is_cancelled() {
            break;
        }
        tracing::info!(duration = ?req.duration, mime_type = %req.mime_type, "processing display request");
        let result = process_request(
            &mut *display,
            &req,
            cfg,
            &work,
            &mut eyecatch_frames,
            &mut eyecatch_loaded,
        );
        // Clear on success, timeout, decode failure, and shutdown. Never reopen a closed window.
        display.clear()?;
        match result {
            Ok(()) => tracing::info!("display done"),
            Err(e) if e.is::<WindowClosedError>() => break,
            Err(_) if shutdown.is_cancelled() => break,
            Err(e) if e.is::<Stopped>() || Instant::now() >= work.deadline => {
                tracing::info!("request deadline exceeded");
            }
            Err(e) => tracing::error!(error = %e, "display error"),
        }
    }
    display.clear()?;
    tracing::info!("worker stopped");
    Ok(())
}

fn process_request(
    display: &mut dyn LedDisplay,
    req: &DisplayRequest,
    cfg: &Config,
    work: &Work<'_>,
    eyecatch_frames: &mut Option<Vec<AnimFrame>>,
    eyecatch_loaded: &mut bool,
) -> anyhow::Result<()> {
    work.check()?;
    if !*eyecatch_loaded {
        if let Some(path) = cfg.eyecatch_path.as_deref().filter(|p| !p.is_empty()) {
            let result = load_eyecatch(path, cfg, work);
            work.check()?;
            match result {
                Ok(frames) => *eyecatch_frames = Some(frames),
                Err(e) => tracing::warn!(error = %e, path, "failed to load eye-catch GIF"),
            }
        }
        *eyecatch_loaded = true;
    }
    work.check()?;
    if let Some(path) = cfg.jingle_path.as_deref().filter(|p| !p.is_empty()) {
        play_jingle(path);
    }
    if let Some(frames) = eyecatch_frames {
        let eye_work = Work {
            shutdown: work.shutdown,
            deadline: Instant::now()
                .checked_add(cfg.eyecatch_duration)
                .unwrap_or(work.deadline)
                .min(work.deadline),
        };
        if cfg.eyecatch_duration > Duration::ZERO {
            match crate::display::show_animated(display, frames, &cfg.image_limits, &eye_work) {
                Err(e) if e.is::<WindowClosedError>() => return Err(e),
                Err(e) if !e.is::<Stopped>() => {
                    tracing::warn!(error = %e, "eye-catch display error");
                }
                _ => {}
            }
            display.clear()?;
        }
    }
    work.check()?;
    if is_gif(&req.mime_type) {
        let frames = crate::decode::gif(&req.image_data, &cfg.image_limits, work)?;
        crate::display::show_animated(display, &frames, &cfg.image_limits, work)
    } else {
        let image = crate::decode::image(&req.image_data, &cfg.image_limits, work)?;
        crate::display::show(
            display,
            &image,
            resolve_display_mode(req.display_mode, &req.mime_type),
            cfg.scroll_interval,
            &cfg.image_limits,
            work,
        )
    }
}

fn load_eyecatch(path: &str, cfg: &Config, work: &Work<'_>) -> anyhow::Result<Vec<AnimFrame>> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    anyhow::ensure!(
        file.metadata()?.is_file(),
        "eye-catch must be a regular file"
    );
    // Match the bounded gRPC payload size; do not read arbitrary local files into memory.
    const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
    anyhow::ensure!(
        file.metadata()?.len() <= MAX_FILE_BYTES,
        "eye-catch file exceeds 4 MiB"
    );
    let mut data = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        work.check()?;
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        anyhow::ensure!(
            data.len() as u64 + count as u64 <= MAX_FILE_BYTES,
            "eye-catch file exceeds 4 MiB"
        );
        data.extend_from_slice(&chunk[..count]);
    }
    crate::decode::gif(&data, &cfg.image_limits, work)
}

fn resolve_display_mode(proto_mode: ProtoDisplayMode, mime_type: &str) -> DisplayMode {
    match proto_mode {
        ProtoDisplayMode::Static => DisplayMode::Static,
        ProtoDisplayMode::Scroll => DisplayMode::ScrollHorizontal,
        ProtoDisplayMode::Unspecified => {
            if is_ppm(mime_type) {
                DisplayMode::ScrollHorizontal
            } else {
                DisplayMode::Static
            }
        }
    }
}

/// Play the WAV file at `path` in a background thread.
///
/// The audio runs concurrently with image display. Errors are logged but do not
/// affect display.
///
/// On Linux, opens the USB audio card directly via the ALSA `alsa` crate using a
/// numeric card index (`plughw:N,0`). This bypasses PipeWire/PulseAudio name
/// resolution, which fails when running as a systemd service without a user session.
/// On other platforms, uses rodio with the system default sink.
fn play_jingle(path: &str) {
    let path = path.to_owned();
    std::thread::spawn(move || {
        let play = || -> anyhow::Result<()> {
            #[cfg(target_os = "linux")]
            {
                play_via_alsa(&path)
            }
            #[cfg(not(target_os = "linux"))]
            {
                use rodio::Source;
                let file = std::fs::File::open(&path)?;
                let source = rodio::Decoder::try_from(std::io::BufReader::new(file))?.buffered();
                let mut sink = rodio::DeviceSinkBuilder::open_default_sink()
                    .map_err(|e| anyhow::anyhow!("failed to open audio sink: {e}"))?;
                sink.log_on_drop(false);
                let player = rodio::Player::connect_new(sink.mixer());
                player.append(source);
                player.sleep_until_end();
                Ok(())
            }
        };
        if let Err(e) = play() {
            tracing::warn!(error = %e, path = %path, "jingle playback failed");
        }
    });
}

/// Play a WAV file directly via ALSA using the USB audio card's numeric index.
///
/// Uses `plughw:N,0` to avoid ALSA name resolution (which fails in systemd services
/// without a user session). Supports 16-bit int, 32-bit int, and 32-bit float WAV.
#[cfg(target_os = "linux")]
fn play_via_alsa(path: &str) -> anyhow::Result<()> {
    use alsa::pcm::{Access, HwParams, PCM};
    use alsa::Direction;

    let idx = usb_audio_card_index()
        .ok_or_else(|| anyhow::anyhow!("USB audio card not found in /proc/asound/cards"))?;
    let device = format!("plughw:{idx},0");
    tracing::debug!(idx, %device, "playing jingle via ALSA");

    let mut reader = hound::WavReader::open(path)
        .map_err(|e| anyhow::anyhow!("failed to open WAV '{path}': {e}"))?;
    let spec = reader.spec();
    tracing::debug!(
        channels = spec.channels,
        sample_rate = spec.sample_rate,
        bits_per_sample = spec.bits_per_sample,
        "WAV spec"
    );

    let pcm = PCM::new(&device, Direction::Playback, false)
        .map_err(|e| anyhow::anyhow!("ALSA open {device}: {e}"))?;

    {
        let hwp = HwParams::any(&pcm).map_err(|e| anyhow::anyhow!("ALSA HwParams: {e}"))?;
        hwp.set_channels(spec.channels as u32)
            .map_err(|e| anyhow::anyhow!("ALSA set_channels: {e}"))?;
        hwp.set_rate(spec.sample_rate, alsa::ValueOr::Nearest)
            .map_err(|e| anyhow::anyhow!("ALSA set_rate: {e}"))?;
        hwp.set_format(wav_to_alsa_format(
            spec.sample_format,
            spec.bits_per_sample,
        )?)
        .map_err(|e| anyhow::anyhow!("ALSA set_format: {e}"))?;
        hwp.set_access(Access::RWInterleaved)
            .map_err(|e| anyhow::anyhow!("ALSA set_access: {e}"))?;
        pcm.hw_params(&hwp)
            .map_err(|e| anyhow::anyhow!("ALSA hw_params: {e}"))?;
    }

    match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Int, 16) => {
            let samples: Vec<i16> = reader
                .samples::<i16>()
                .collect::<Result<_, _>>()
                .map_err(|e| anyhow::anyhow!("WAV decode: {e}"))?;
            pcm.io_i16()
                .map_err(|e| anyhow::anyhow!("ALSA io: {e}"))?
                .writei(&samples)
                .map_err(|e| anyhow::anyhow!("ALSA write: {e}"))?;
        }
        (hound::SampleFormat::Int, 32) => {
            let samples: Vec<i32> = reader
                .samples::<i32>()
                .collect::<Result<_, _>>()
                .map_err(|e| anyhow::anyhow!("WAV decode: {e}"))?;
            pcm.io_i32()
                .map_err(|e| anyhow::anyhow!("ALSA io: {e}"))?
                .writei(&samples)
                .map_err(|e| anyhow::anyhow!("ALSA write: {e}"))?;
        }
        (hound::SampleFormat::Float, 32) => {
            let samples: Vec<f32> = reader
                .samples::<f32>()
                .collect::<Result<_, _>>()
                .map_err(|e| anyhow::anyhow!("WAV decode: {e}"))?;
            pcm.io_f32()
                .map_err(|e| anyhow::anyhow!("ALSA io: {e}"))?
                .writei(&samples)
                .map_err(|e| anyhow::anyhow!("ALSA write: {e}"))?;
        }
        _ => anyhow::bail!(
            "unsupported WAV format: {:?} {}-bit (supported: i16, i32, f32)",
            spec.sample_format,
            spec.bits_per_sample
        ),
    }

    pcm.drain()
        .map_err(|e| anyhow::anyhow!("ALSA drain: {e}"))?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn wav_to_alsa_format(fmt: hound::SampleFormat, bits: u16) -> anyhow::Result<alsa::pcm::Format> {
    match (fmt, bits) {
        (hound::SampleFormat::Int, 16) => Ok(alsa::pcm::Format::S16LE),
        (hound::SampleFormat::Int, 32) => Ok(alsa::pcm::Format::S32LE),
        (hound::SampleFormat::Float, 32) => Ok(alsa::pcm::Format::FloatLE),
        _ => anyhow::bail!(
            "unsupported WAV format: {:?} {}-bit (supported: i16, i32, f32)",
            fmt,
            bits
        ),
    }
}

/// Read `/proc/asound/cards` and return the numeric index of the first USB audio device.
/// Format: " N [ShortName    ]: driver - Full Name"
/// Using a numeric index avoids snd_config_get_card name-resolution, which fails in
/// some environments (e.g. as a child process of a systemd service with cleared env).
#[cfg(target_os = "linux")]
fn usb_audio_card_index() -> Option<u32> {
    let content = std::fs::read_to_string("/proc/asound/cards").ok()?;
    for line in content.lines() {
        if line.contains("USB") {
            let trimmed = line.trim_start();
            let idx_str = trimmed.split_whitespace().next()?;
            return idx_str.parse().ok();
        }
    }
    None
}

fn is_gif(mime_type: &str) -> bool {
    mime_type.eq_ignore_ascii_case("image/gif")
}

fn is_ppm(mime_type: &str) -> bool {
    mime_type.contains("portable-pixmap") || mime_type.contains("ppm") || mime_type.contains("pnm")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{self, FakeDisplay, State};
    use std::{cell::RefCell, rc::Rc};

    fn request() -> DisplayRequest {
        DisplayRequest {
            image_data: test_support::png(4, 2),
            mime_type: "image/png".into(),
            duration: Duration::from_secs(30),
            display_mode: ProtoDisplayMode::Static,
        }
    }

    #[test]
    fn cancellation_during_display_discards_full_queue() {
        let shutdown = Shutdown::new();
        let state = Rc::new(RefCell::new(State {
            cancel_on_render: Some(shutdown.clone()),
            ..State::default()
        }));
        let (tx, rx) = std::sync::mpsc::sync_channel(10);
        for _ in 0..10 {
            tx.send(request()).unwrap();
        }
        run_loop(
            Box::new(FakeDisplay(state.clone())),
            rx,
            &test_support::config(),
            &shutdown,
        )
        .unwrap();
        assert_eq!(state.borrow().renders, 1);
        assert!(state.borrow().clears > 0);
        assert!(tx.send(request()).is_err());
    }

    #[test]
    fn window_close_stops_server_while_idle_or_rendering() {
        for idle in [true, false] {
            let shutdown = Shutdown::new();
            let state = Rc::new(RefCell::new(State {
                close_on_poll: idle,
                close_on_render: !idle,
                ..State::default()
            }));
            let (tx, rx) = std::sync::mpsc::sync_channel(10);
            if !idle {
                tx.send(request()).unwrap();
            }
            run_loop(
                Box::new(FakeDisplay(state.clone())),
                rx,
                &test_support::config(),
                &shutdown,
            )
            .unwrap();
            assert!(shutdown.is_cancelled());
            assert_eq!(state.borrow().renders, usize::from(!idle));
        }
    }

    #[test]
    fn idle_worker_observes_external_shutdown() {
        let shutdown = Shutdown::new();
        let trigger = shutdown.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(25));
            trigger.cancel();
        });
        let state = Rc::new(RefCell::new(State::default()));
        let (_tx, rx) = std::sync::mpsc::sync_channel(10);
        let start = Instant::now();
        run_loop(
            Box::new(FakeDisplay(state.clone())),
            rx,
            &test_support::config(),
            &shutdown,
        )
        .unwrap();
        thread.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(state.borrow().polls > 0);
    }

    #[test]
    fn rejected_image_does_not_block_next_request() {
        let shutdown = Shutdown::new();
        let state = Rc::new(RefCell::new(State {
            cancel_on_render: Some(shutdown.clone()),
            ..State::default()
        }));
        let (tx, rx) = std::sync::mpsc::sync_channel(10);
        let mut invalid = request();
        invalid.image_data = test_support::gif(5, 1, 1);
        invalid.mime_type = "image/gif".into();
        tx.send(invalid).unwrap();
        tx.send(request()).unwrap();
        let mut cfg = test_support::config();
        cfg.image_limits.max_dimension = 4;
        run_loop(Box::new(FakeDisplay(state.clone())), rx, &cfg, &shutdown).unwrap();
        assert_eq!(state.borrow().renders, 1);
        assert!(state.borrow().clears >= 2);
    }

    #[test]
    fn eyecatch_uses_request_deadline_and_skips_main_decode_when_expired() {
        let shutdown = Shutdown::new();
        let state = Rc::new(RefCell::new(State::default()));
        let mut display = FakeDisplay(state.clone());
        let cfg = test_support::config();
        let work = Work {
            shutdown: &shutdown,
            deadline: Instant::now() + Duration::from_millis(25),
        };
        let mut frames = Some(vec![AnimFrame {
            image: image::DynamicImage::new_rgb8(4, 2),
            delay: Duration::from_secs(5),
        }]);
        let mut req = request();
        req.image_data = b"invalid main image".to_vec();
        let start = Instant::now();
        let err =
            process_request(&mut display, &req, &cfg, &work, &mut frames, &mut true).unwrap_err();
        assert!(err.is::<Stopped>());
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(state.borrow().renders > 0);
    }
}

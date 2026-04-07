use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};


use crate::display::{AnimFrame, DisplayMode, LedDisplay};
use crate::proto::DisplayMode as ProtoDisplayMode;

/// A request to display an image, as queued by the gRPC service.
pub struct DisplayRequest {
    /// Raw image bytes received from the client.
    pub image_data: Vec<u8>,
    /// MIME type of the image (e.g. `image/png`, `image/gif`).
    pub mime_type: String,
    /// How long to display the image.
    pub duration: Duration,
    /// Display mode requested by the client.
    pub display_mode: ProtoDisplayMode,
}

/// Run the display loop on the calling thread.
///
/// Processes [`DisplayRequest`]s from `rx` sequentially until the channel is closed.
/// Each request is subject to `worker_timeout`; if display takes longer, it is cut short.
///
/// **Must be called on the main thread on macOS** because minifb uses Cocoa.
pub fn run_loop(
    mut display: Box<dyn LedDisplay>,
    rx: Receiver<DisplayRequest>,
    worker_timeout: Duration,
    scroll_interval: Duration,
    jingle_path: Option<String>,
    eyecatch_path: Option<String>,
    eyecatch_duration: Duration,
) {
    // Pre-decode the eye-catch GIF once so it is ready for every request.
    let eyecatch_frames: Option<Vec<AnimFrame>> = eyecatch_path.as_deref().and_then(|path| {
        match std::fs::read(path).map_err(anyhow::Error::from).and_then(|data| decode_gif(&data)) {
            Ok(frames) => Some(frames),
            Err(e) => {
                tracing::warn!(error = %e, path = %path, "failed to load eye-catch GIF");
                None
            }
        }
    });

    for req in rx.iter() {
        tracing::info!(
            duration = ?req.duration,
            mime_type = %req.mime_type,
            "processing display request"
        );

        // Play jingle regardless of whether an eye-catch is configured.
        if let Some(ref path) = jingle_path {
            play_jingle(path);
        }
        // Show eye-catch GIF if configured (jingle plays concurrently above).
        if let Some(ref frames) = eyecatch_frames {
            let eyecatch_deadline = Instant::now() + eyecatch_duration;
            if let Err(e) = crate::display::show_animated(&mut *display, frames, eyecatch_deadline) {
                tracing::warn!(error = %e, "eye-catch display error");
            }
        }

        let deadline = Instant::now() + req.duration.min(worker_timeout);
        let result = if is_gif(&req.mime_type) {
            if req.display_mode != ProtoDisplayMode::Unspecified {
                tracing::warn!(
                    display_mode = ?req.display_mode,
                    "display_mode is ignored for animated GIF"
                );
            }
            run_animated(&mut *display, &req.image_data, deadline)
        } else {
            let mode = resolve_display_mode(req.display_mode, &req.mime_type);
            run_static(&mut *display, &req.image_data, mode, deadline, scroll_interval)
        };

        match result {
            Ok(()) => tracing::info!("display done"),
            Err(e) => tracing::error!(error = %e, "display error"),
        }
    }
    tracing::info!("worker stopped");
}

fn run_static(
    display: &mut dyn LedDisplay,
    data: &[u8],
    mode: DisplayMode,
    deadline: Instant,
    scroll_interval: Duration,
) -> anyhow::Result<()> {
    let img = decode_image(data)?;
    crate::display::show(display, &img, deadline, mode, scroll_interval)
}

fn run_animated(
    display: &mut dyn LedDisplay,
    data: &[u8],
    deadline: Instant,
) -> anyhow::Result<()> {
    let frames = decode_gif(data)?;
    crate::display::show_animated(display, &frames, deadline)
}

/// Resolve the effective [`DisplayMode`] from the proto request field and mime_type fallback.
///
/// When `proto_mode` is `Unspecified`, the mode is inferred from `mime_type`:
/// PPM/PNM files default to [`DisplayMode::ScrollHorizontal`], all others to [`DisplayMode::Static`].
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

fn decode_image(data: &[u8]) -> anyhow::Result<image::DynamicImage> {
    image::load_from_memory(data).map_err(Into::into)
}

/// Decode an animated GIF into a sequence of [`AnimFrame`]s.
///
/// Frame delays of 0 default to 100 ms. Delays below 10 ms are clamped to 10 ms.
fn decode_gif(data: &[u8]) -> anyhow::Result<Vec<AnimFrame>> {
    use image::codecs::gif::GifDecoder;
    use image::AnimationDecoder;

    let cursor = std::io::Cursor::new(data);
    let decoder = GifDecoder::new(cursor)?;
    let frames = decoder.into_frames().collect_frames()?;

    if frames.is_empty() {
        anyhow::bail!("GIF has no frames");
    }

    frames
        .into_iter()
        .map(|f| {
            // `numer_denom_ms()` returns (u32, u32); cast to u64 to avoid overflow.
            let (num, den) = f.delay().numer_denom_ms();
            let delay_ms = if den == 0 || num == 0 {
                100u32
            } else {
                // Use u64 to avoid overflow, then clamp. `div_ceil` avoids the
                // manual `(num + den - 1) / den` idiom flagged by clippy.
                let ms = (num as u64).div_ceil(den as u64);
                if ms > 60_000 {
                    tracing::warn!(delay_ms = ms, "GIF frame delay unusually large, clamping to 60 s");
                }
                (ms.min(60_000u64) as u32).max(10)
            };
            Ok(AnimFrame {
                image: image::DynamicImage::ImageRgba8(f.into_buffer()),
                delay: Duration::from_millis(delay_ms as u64),
            })
        })
        .collect()
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
        let hwp = HwParams::any(&pcm)
            .map_err(|e| anyhow::anyhow!("ALSA HwParams: {e}"))?;
        hwp.set_channels(spec.channels as u32)
            .map_err(|e| anyhow::anyhow!("ALSA set_channels: {e}"))?;
        hwp.set_rate(spec.sample_rate, alsa::ValueOr::Nearest)
            .map_err(|e| anyhow::anyhow!("ALSA set_rate: {e}"))?;
        hwp.set_format(wav_to_alsa_format(spec.sample_format, spec.bits_per_sample)?)
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
    mime_type.contains("portable-pixmap")
        || mime_type.contains("ppm")
        || mime_type.contains("pnm")
}

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
/// On Linux, uses `aplay` with a dynamically generated minimal ALSA config
/// (written to a temp file and passed via `ALSA_CONFIG_PATH`) to bypass
/// ALSA's `snd_config_get_card()` name resolution, which fails when the
/// PipeWire ALSA plugin cannot connect to a user session (e.g. as a child of a
/// systemd service). Using an integer `card N` in the generated config avoids
/// the name resolution path entirely.
/// On other platforms, uses rodio with the system default sink.
fn play_jingle(path: &str) {
    let path = path.to_owned();
    std::thread::spawn(move || {
        let play = || -> anyhow::Result<()> {
            #[cfg(target_os = "linux")]
            {
                use std::io::Write as _;

                let idx = usb_audio_card_index().ok_or_else(|| anyhow::anyhow!("USB audio card not found in /proc/asound/cards"))?;
                tracing::debug!(idx, "playing jingle via aplay on USB card");

                // Write a minimal ALSA config to a per-invocation temp file to avoid
                // snd_config_get_card() name resolution failures (common when running
                // as a systemd service) and to eliminate write-then-read races that
                // would occur if a single shared file were used.
                let alsa_conf = format!(
                    "pcm.usbplay {{\n  type plug\n  slave.pcm {{\n    type hw\n    card {idx}\n    device 0\n  }}\n}}\nctl.usbplay {{\n  type hw\n  card {idx}\n}}\n"
                );
                let mut conf_file = tempfile::NamedTempFile::new()
                    .map_err(|e| anyhow::anyhow!("failed to create temp ALSA config: {e}"))?;
                conf_file.write_all(alsa_conf.as_bytes())
                    .map_err(|e| anyhow::anyhow!("failed to write ALSA config: {e}"))?;
                conf_file.flush()
                    .map_err(|e| anyhow::anyhow!("failed to flush ALSA config: {e}"))?;

                let output = std::process::Command::new("aplay")
                    .env("ALSA_CONFIG_PATH", conf_file.path())
                    .arg("-D").arg("usbplay")
                    .arg(&path)
                    .output()
                    .map_err(|e| anyhow::anyhow!("failed to run aplay: {e}"))?;
                // conf_file is dropped here — temp file is deleted after aplay exits.
                drop(conf_file);

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    anyhow::bail!("aplay exited with {}: {}", output.status, stderr);
                }
                Ok(())
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

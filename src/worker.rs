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

        // Show eye-catch GIF and play jingle simultaneously.
        if let Some(ref frames) = eyecatch_frames {
            if let Some(ref path) = jingle_path {
                play_jingle(path);
            }
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
            let (num, den) = f.delay().numer_denom_ms();
            let delay_ms = if den == 0 || num == 0 {
                100
            } else {
                (num / den).max(10)
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
/// affect display. The background thread owns the `OutputStream` for the duration
/// of playback; dropping it early would stop audio.
fn play_jingle(path: &str) {
    let path = path.to_owned();
    std::thread::spawn(move || {
        let play = || -> anyhow::Result<()> {
            let mut sink = rodio::DeviceSinkBuilder::open_default_sink()?;
            sink.log_on_drop(false);
            let player = rodio::Player::connect_new(sink.mixer());
            let file = std::fs::File::open(&path)?;
            player.append(rodio::Decoder::try_from(file)?);
            player.sleep_until_end();
            Ok(())
        };
        if let Err(e) = play() {
            tracing::warn!(error = %e, path = %path, "jingle playback failed");
        }
    });
}

fn is_gif(mime_type: &str) -> bool {
    mime_type.contains("gif")
}

fn is_ppm(mime_type: &str) -> bool {
    mime_type.contains("portable-pixmap")
        || mime_type.contains("ppm")
        || mime_type.contains("pnm")
}

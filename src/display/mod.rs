use anyhow::Result;
use image::{DynamicImage, GenericImageView};
use std::time::{Duration, Instant};

use crate::config::Config;

// ---------------------------------------------------------------------------
// Image scaling helpers
// ---------------------------------------------------------------------------

/// Scale image to panel width, preserving aspect ratio.
/// - If the scaled height exceeds the panel height, crop vertically from the center.
/// - If shorter, pad top and bottom with black.
fn resize_to_width(img: &DynamicImage, cols: u32, rows: u32) -> DynamicImage {
    let scale = cols as f32 / img.width() as f32;
    let new_h = ((img.height() as f32 * scale) as u32).max(1);

    let resized = img.resize_exact(cols, new_h, image::imageops::FilterType::Nearest);

    if new_h <= rows {
        let mut canvas = DynamicImage::new_rgb8(cols, rows);
        let y = (rows - new_h) / 2;
        image::imageops::overlay(&mut canvas, &resized, 0, y as i64);
        canvas
    } else {
        let y_offset = (new_h - rows) / 2;
        resized.crop_imm(0, y_offset, cols, rows)
    }
}

/// Scale image to panel height, preserving aspect ratio.
/// - If the scaled width exceeds the panel width, crop horizontally from the center.
/// - If shorter, pad left and right with black.
fn resize_to_height(img: &DynamicImage, cols: u32, rows: u32) -> DynamicImage {
    let scale = rows as f32 / img.height() as f32;
    let new_w = ((img.width() as f32 * scale) as u32).max(1);

    let resized = img.resize_exact(new_w, rows, image::imageops::FilterType::Nearest);

    if new_w <= cols {
        let mut canvas = DynamicImage::new_rgb8(cols, rows);
        let x = (cols - new_w) / 2;
        image::imageops::overlay(&mut canvas, &resized, x as i64, 0);
        canvas
    } else {
        let x_offset = (new_w - cols) / 2;
        resized.crop_imm(x_offset, 0, cols, rows)
    }
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single frame decoded from an animated GIF.
pub struct AnimFrame {
    /// Decoded frame image.
    pub image: DynamicImage,
    /// Duration for which this frame should be displayed.
    pub delay: Duration,
}

/// Controls how a still image is rendered on the LED panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DisplayMode {
    /// Show image statically (scaled to panel width).
    Static,
    /// Scroll image horizontally (scaled to panel height, scrolled left).
    ScrollHorizontal,
}

/// Low-level display backend.
///
/// Implementations are responsible only for rendering a single frame of pixels
/// and clearing the panel. Timing (frame rate, scroll speed) is handled by the
/// shared [`show`] / [`show_animated`] functions.
pub trait LedDisplay {
    /// Number of LED rows on the panel.
    fn rows(&self) -> usize;
    /// Number of LED columns on the panel.
    fn cols(&self) -> usize;

    /// Render one frame of pixels.
    ///
    /// `pixels` is a row-major slice of `(r, g, b)` values with `rows * cols` entries.
    ///
    /// **Blocking contract**: implementations are expected to block for approximately
    /// one frame duration before returning (e.g. the emulator sleeps for
    /// [`FRAME_INTERVAL`], the RPi backend blocks on vsync). The display loops in
    /// [`show`] and [`show_animated`] rely on this behaviour to cap CPU usage.
    ///
    /// Returns [`WindowClosedError`] (wrapped in `anyhow::Error`) when the
    /// emulator window has been closed; callers should exit gracefully on this error.
    fn render_frame(&mut self, pixels: &[(u8, u8, u8)]) -> Result<()>;

    /// Clear the panel to all black.
    fn clear(&mut self) -> Result<()>;
}

// ---------------------------------------------------------------------------
// Shared display loops
// ---------------------------------------------------------------------------

/// Display refresh interval (~60 fps).
const FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// Show a still image on the panel until `deadline`, then clear.
///
/// - [`DisplayMode::Static`]: image is scaled to panel width and shown unchanged.
/// - [`DisplayMode::ScrollHorizontal`]: image is scaled to panel height and scrolled left
///   at a rate of 1 pixel per `scroll_interval`.
///
/// # Note on scroll speed
/// The display loop refreshes at [`FRAME_INTERVAL`] (≈60 fps). If `scroll_interval` is shorter
/// than `FRAME_INTERVAL`, the scroll still advances only 1 pixel per loop iteration, so the
/// effective minimum scroll interval is capped at `FRAME_INTERVAL` (16 ms).
pub fn show(
    display: &mut dyn LedDisplay,
    img: &DynamicImage,
    deadline: Instant,
    mode: DisplayMode,
    scroll_interval: Duration,
) -> Result<()> {
    let rows = display.rows();
    let cols = display.cols();

    let panel = match mode {
        DisplayMode::Static => resize_to_width(img, cols as u32, rows as u32),
        DisplayMode::ScrollHorizontal => {
            // Scale to panel height; full width is preserved for scrolling.
            let scale = rows as f32 / img.height() as f32;
            let new_w = ((img.width() as f32 * scale) as u32).max(1);
            img.resize_exact(new_w, rows as u32, image::imageops::FilterType::Nearest)
        }
    };

    let img_w = panel.width() as usize;
    let mut x_offset = 0usize;
    let mut last_scroll = Instant::now();

    // Pre-allocate pixel buffer once; reused across frames to avoid repeated heap allocation.
    let mut pixels = Vec::with_capacity(rows * cols);
    fill_pixels(&mut pixels, &panel, x_offset, rows, cols);

    while Instant::now() < deadline {
        if let Err(e) = display.render_frame(&pixels) {
            if e.is::<WindowClosedError>() {
                return Ok(());
            }
            return Err(e);
        }

        if mode == DisplayMode::ScrollHorizontal && last_scroll.elapsed() >= scroll_interval {
            x_offset = (x_offset + 1) % img_w;
            last_scroll = Instant::now();
            fill_pixels(&mut pixels, &panel, x_offset, rows, cols);
        }
    }

    display.clear()
}

/// Cycle through animated GIF frames until `deadline`, then clear.
///
/// Each frame is displayed for its embedded delay duration. If the deadline
/// arrives mid-frame the loop exits early. After the loop, a [`FRAME_INTERVAL`]
/// sleep ensures the final frame is actually rendered before clearing.
pub fn show_animated(
    display: &mut dyn LedDisplay,
    frames: &[AnimFrame],
    deadline: Instant,
) -> Result<()> {
    if frames.is_empty() {
        return Ok(());
    }

    let rows = display.rows();
    let cols = display.cols();

    // Pre-resize all frames once to avoid repeated allocation in the loop.
    type PixelBuf = Vec<(u8, u8, u8)>;
    let panels: Vec<(PixelBuf, Duration)> = frames
        .iter()
        .map(|f| {
            let panel = resize_to_height(&f.image, cols as u32, rows as u32);
            let mut pixels = Vec::with_capacity(rows * cols);
            fill_pixels(&mut pixels, &panel, 0, rows, cols);
            (pixels, f.delay)
        })
        .collect();

    let mut frame_idx = 0;
    while Instant::now() < deadline {
        let (pixels, delay) = &panels[frame_idx % panels.len()];
        let frame_end = (Instant::now() + *delay).min(deadline);

        while Instant::now() < frame_end {
            if let Err(e) = display.render_frame(pixels) {
                if e.is::<WindowClosedError>() {
                    return Ok(());
                }
                return Err(e);
            }
            let remaining = frame_end.saturating_duration_since(Instant::now());
            // Sleep only if render_frame returns quickly (emulator).
            // On RPi, update_on_vsync already blocks for the frame duration.
            if remaining > Duration::ZERO {
                std::thread::sleep(remaining.min(FRAME_INTERVAL));
            }
        }

        frame_idx += 1;
    }

    display.clear()
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn fill_pixels(
    buf: &mut Vec<(u8, u8, u8)>,
    panel: &DynamicImage,
    x_offset: usize,
    rows: usize,
    cols: usize,
) {
    let img_w = panel.width() as usize;
    buf.clear();
    buf.extend((0..rows).flat_map(|y| {
        (0..cols).map(move |x| {
            let src_x = (x_offset + x) % img_w;
            let p = panel.get_pixel(src_x as u32, y as u32);
            (p[0], p[1], p[2])
        })
    }));
}

// ---------------------------------------------------------------------------
// Error sentinels
// ---------------------------------------------------------------------------

/// Sentinel error returned by [`LedDisplay::render_frame`] when the display
/// window has been closed by the user.
///
/// Callers detect this via [`anyhow::Error::is::<WindowClosedError>()`] and
/// exit display loops cleanly without propagating the error.
#[derive(Debug, thiserror::Error)]
#[error("window closed")]
pub struct WindowClosedError;

// ---------------------------------------------------------------------------
// Backend selection
// ---------------------------------------------------------------------------

#[cfg(not(feature = "rpi"))]
mod emulator;

#[cfg(feature = "rpi")]
mod rpi;

/// Create the appropriate display backend based on compiled features.
///
/// - Without `--features rpi`: uses the [`emulator`] backend (minifb window).
/// - With `--features rpi`: uses the [`rpi`] backend (rpi-led-panel hardware).
pub fn create(cfg: &Config) -> Result<Box<dyn LedDisplay>> {
    #[cfg(feature = "rpi")]
    {
        return rpi::RpiDisplay::new(cfg).map(|d| Box::new(d) as Box<dyn LedDisplay>);
    }
    #[cfg(not(feature = "rpi"))]
    {
        Ok(Box::new(emulator::EmulatorDisplay::new(cfg)))
    }
}

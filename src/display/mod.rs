use anyhow::Result;
use image::{DynamicImage, GenericImageView};
use std::num::NonZeroU32;
use std::time::{Duration, Instant};

use crate::config::{Config, ImageLimits};
use crate::shutdown::Work;

pub struct AnimFrame {
    pub image: DynamicImage,
    pub delay: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DisplayMode {
    Static,
    ScrollHorizontal,
}

#[derive(Debug, Clone, Copy)]
pub enum DisplayLimit {
    Duration(Duration),
    ScrollCycles {
        cycles: NonZeroU32,
        min_display_duration: Duration,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct DisplayStats {
    pub completed_cycles: u64,
    pub prepared_width_px: u32,
    pub elapsed: Duration,
}

/// Backends render on the main thread and block for approximately one refresh.
pub trait LedDisplay {
    fn rows(&self) -> usize;
    fn cols(&self) -> usize;
    fn render_frame(&mut self, pixels: &[(u8, u8, u8)]) -> Result<()>;

    /// Pump events without opening a window while idle.
    fn poll_events(&mut self) -> Result<()> {
        Ok(())
    }

    fn clear(&mut self) -> Result<()>;
}

#[derive(Clone, Copy)]
enum Scale {
    Width,
    Height,
    Scroll,
}

/// Sample directly into the visible canvas, avoiding oversized resize intermediates.
fn prepare(
    display: &mut dyn LedDisplay,
    img: &DynamicImage,
    scale: Scale,
    limits: &ImageLimits,
    work: &Work<'_>,
) -> Result<DynamicImage> {
    work.check()?;
    let (cols, rows) = (display.cols() as u32, display.rows() as u32);
    limits.check_panel(cols, rows)?;
    limits.check_dimensions(img.width(), img.height())?;
    let (scaled_w, scaled_h) = match scale {
        Scale::Width => (
            u64::from(cols),
            (u64::from(img.height()) * u64::from(cols) / u64::from(img.width())).max(1),
        ),
        Scale::Height | Scale::Scroll => (
            (u64::from(img.width()) * u64::from(rows) / u64::from(img.height())).max(1),
            u64::from(rows),
        ),
    };
    let width = if matches!(scale, Scale::Scroll) {
        u32::try_from(scaled_w)?
    } else {
        cols
    };
    // Include both the image and the row-major RGB rendering buffer.
    limits.check_render_buffers(
        cols,
        rows,
        (u64::from(width) + u64::from(cols)) * u64::from(rows) * 3,
    )?;
    let mut canvas = image::RgbImage::new(width, rows);
    let crop_x = scaled_w.saturating_sub(u64::from(width)) / 2;
    let crop_y = scaled_h.saturating_sub(u64::from(rows)) / 2;
    let pad_x = u64::from(width).saturating_sub(scaled_w) / 2;
    let pad_y = u64::from(rows).saturating_sub(scaled_h) / 2;
    for y in 0..rows {
        work.check()?;
        if y % 32 == 0 {
            display.poll_events()?;
        }
        for x in 0..width {
            let Some(sx) = u64::from(x).checked_sub(pad_x) else {
                continue;
            };
            let Some(sy) = u64::from(y).checked_sub(pad_y) else {
                continue;
            };
            if sx >= scaled_w || sy >= scaled_h {
                continue;
            }
            let source_x =
                ((2 * (sx + crop_x) + 1) * u64::from(img.width()) / (2 * scaled_w)) as u32;
            let source_y =
                ((2 * (sy + crop_y) + 1) * u64::from(img.height()) / (2 * scaled_h)) as u32;
            let pixel = img.get_pixel(
                source_x.min(img.width() - 1),
                source_y.min(img.height() - 1),
            );
            let alpha = u16::from(pixel[3]);
            canvas.put_pixel(
                x,
                y,
                image::Rgb([
                    (u16::from(pixel[0]) * alpha / 255) as u8,
                    (u16::from(pixel[1]) * alpha / 255) as u8,
                    (u16::from(pixel[2]) * alpha / 255) as u8,
                ]),
            );
        }
    }
    work.check()?;
    Ok(DynamicImage::ImageRgb8(canvas))
}

pub fn show(
    display: &mut dyn LedDisplay,
    img: &DynamicImage,
    mode: DisplayMode,
    scroll_interval: Duration,
    limit: DisplayLimit,
    limits: &ImageLimits,
    work: &Work<'_>,
) -> Result<DisplayStats> {
    let panel = prepare(
        display,
        img,
        match mode {
            DisplayMode::Static => Scale::Width,
            DisplayMode::ScrollHorizontal => Scale::Scroll,
        },
        limits,
        work,
    )?;
    let (rows, cols) = (display.rows(), display.cols());
    let mut pixels = Vec::with_capacity(rows * cols);
    let mut offset = 0usize;
    fill_pixels(&mut pixels, &panel, offset, rows, cols);
    work.check()?;
    let started = Instant::now();
    let mut position_started = started;
    let mut completed_cycles = 0u64;
    let display_deadline = match limit {
        DisplayLimit::Duration(duration) => Some(
            started
                .checked_add(duration)
                .unwrap_or(work.deadline)
                .min(work.deadline),
        ),
        DisplayLimit::ScrollCycles { .. } => None,
    };

    loop {
        work.check()?;
        let now = Instant::now();
        if display_deadline.is_some_and(|deadline| now >= deadline)
            || cycles_satisfied(limit, completed_cycles, now.duration_since(started))
        {
            return Ok(DisplayStats {
                completed_cycles,
                prepared_width_px: panel.width(),
                elapsed: now.duration_since(started),
            });
        }

        display.render_frame(&pixels)?;
        work.check()?;
        let now = Instant::now();
        let elapsed = now.duration_since(started);
        if cycles_satisfied(limit, completed_cycles, elapsed) {
            return Ok(DisplayStats {
                completed_cycles,
                prepared_width_px: panel.width(),
                elapsed,
            });
        }

        if mode == DisplayMode::ScrollHorizontal
            && now.duration_since(position_started) >= scroll_interval
        {
            if offset + 1 == panel.width() as usize {
                completed_cycles = completed_cycles.saturating_add(1);
                offset = 0;
            } else {
                offset += 1;
            }
            position_started = now;
            work.check()?;
            if cycles_satisfied(limit, completed_cycles, elapsed) {
                return Ok(DisplayStats {
                    completed_cycles,
                    prepared_width_px: panel.width(),
                    elapsed,
                });
            }
            fill_pixels(&mut pixels, &panel, offset, rows, cols);
        }
    }
}

fn cycles_satisfied(limit: DisplayLimit, completed_cycles: u64, elapsed: Duration) -> bool {
    match limit {
        DisplayLimit::Duration(_) => false,
        DisplayLimit::ScrollCycles {
            cycles,
            min_display_duration,
        } => completed_cycles >= u64::from(cycles.get()) && elapsed >= min_display_duration,
    }
}

pub fn show_animated(
    display: &mut dyn LedDisplay,
    frames: &[AnimFrame],
    display_duration: Duration,
    limits: &ImageLimits,
    work: &Work<'_>,
) -> Result<()> {
    work.check()?;
    if frames.is_empty() {
        return Ok(());
    }
    let (rows, cols) = (display.rows(), display.cols());
    limits.check_panel(cols as u32, rows as u32)?;
    let bytes = (rows as u64)
        .checked_mul(cols as u64)
        .and_then(|n| n.checked_mul(3))
        .and_then(|n| n.checked_mul(frames.len() as u64 + 1))
        .ok_or_else(|| anyhow::anyhow!("animation render size overflow"))?;
    limits.check_render_buffers(cols as u32, rows as u32, bytes)?;
    let mut panels = Vec::with_capacity(frames.len());
    for frame in frames {
        let panel = prepare(display, &frame.image, Scale::Height, limits, work)?;
        let mut pixels = Vec::with_capacity(rows * cols);
        fill_pixels(&mut pixels, &panel, 0, rows, cols);
        panels.push((pixels, frame.delay));
    }
    let display_work = display_work(work, display_duration);
    display_work.check()?;
    let mut frame_idx = 0;
    while Instant::now() < display_work.deadline {
        work.check()?;
        let (pixels, delay) = &panels[frame_idx];
        let frame_end = Instant::now()
            .checked_add(*delay)
            .unwrap_or(display_work.deadline)
            .min(display_work.deadline);
        while Instant::now() < frame_end {
            work.check()?;
            display.render_frame(pixels)?;
        }
        frame_idx = (frame_idx + 1) % panels.len();
    }
    Ok(())
}

fn display_work<'a>(work: &Work<'a>, duration: Duration) -> Work<'a> {
    Work {
        shutdown: work.shutdown,
        deadline: Instant::now()
            .checked_add(duration)
            .unwrap_or(work.deadline)
            .min(work.deadline),
    }
}

fn fill_pixels(
    buf: &mut Vec<(u8, u8, u8)>,
    panel: &DynamicImage,
    offset: usize,
    rows: usize,
    cols: usize,
) {
    buf.clear();
    buf.extend((0..rows).flat_map(|y| {
        (0..cols).map(move |x| {
            let p = panel.get_pixel(((offset + x) % panel.width() as usize) as u32, y as u32);
            (p[0], p[1], p[2])
        })
    }));
}

/// Propagated to the worker to stop the server when the window closes.
#[derive(Debug, thiserror::Error)]
#[error("window closed")]
pub struct WindowClosedError;

#[cfg(not(feature = "rpi"))]
mod emulator;

#[cfg(feature = "rpi")]
mod rpi;

/// Select the RPi backend when its feature is enabled; otherwise use the emulator.
pub fn create(cfg: &Config) -> Result<Box<dyn LedDisplay>> {
    #[cfg(feature = "rpi")]
    {
        rpi::RpiDisplay::new(cfg).map(|d| Box::new(d) as Box<dyn LedDisplay>)
    }
    #[cfg(not(feature = "rpi"))]
    {
        Ok(Box::new(emulator::EmulatorDisplay::new(cfg)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        shutdown::Shutdown,
        test_support::{FakeDisplay, State},
    };
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn static_crop_avoids_allocating_full_scaled_image() {
        let shutdown = Shutdown::new();
        let work = Work {
            shutdown: &shutdown,
            deadline: Instant::now() + Duration::from_secs(1),
        };
        let mut display = FakeDisplay(Rc::new(RefCell::new(State::default())));
        let img = DynamicImage::new_rgb8(1, 4096);
        let limits = ImageLimits {
            max_render_bytes: 4096,
            ..ImageLimits::default()
        };
        let panel = prepare(&mut display, &img, Scale::Width, &limits, &work).unwrap();
        assert_eq!(panel.dimensions(), (4, 2));
    }

    #[test]
    fn wide_scroll_is_rejected_before_allocating_buffer() {
        let shutdown = Shutdown::new();
        let work = Work {
            shutdown: &shutdown,
            deadline: Instant::now() + Duration::from_secs(1),
        };
        let mut display = FakeDisplay(Rc::new(RefCell::new(State::default())));
        let limits = ImageLimits {
            max_render_bytes: 4096,
            ..ImageLimits::default()
        };
        let err = prepare(
            &mut display,
            &DynamicImage::new_rgb8(4096, 1),
            Scale::Scroll,
            &limits,
            &work,
        )
        .unwrap_err();
        assert!(err.to_string().contains("MAX_RENDER_BYTES"));
    }

    #[test]
    fn display_duration_starts_after_preparation() {
        let shutdown = Shutdown::new();
        let work = Work {
            shutdown: &shutdown,
            deadline: Instant::now() + Duration::from_secs(1),
        };
        let state = Rc::new(RefCell::new(State {
            poll_delay: Duration::from_millis(40),
            ..State::default()
        }));
        let mut display = FakeDisplay(state.clone());
        let started = Instant::now();
        show(
            &mut display,
            &DynamicImage::new_rgb8(4, 2),
            DisplayMode::Static,
            Duration::from_millis(1),
            DisplayLimit::Duration(Duration::from_millis(40)),
            &ImageLimits::default(),
            &work,
        )
        .unwrap();

        assert!(started.elapsed() >= Duration::from_millis(75));
        assert!(state.borrow().renders > 0);
    }

    #[test]
    fn cycle_limit_renders_exact_offsets_without_an_extra_frame() {
        let shutdown = Shutdown::new();
        let work = Work {
            shutdown: &shutdown,
            deadline: Instant::now() + Duration::from_secs(1),
        };
        let state = Rc::new(RefCell::new(State::default()));
        let mut display = FakeDisplay(state.clone());
        let image = DynamicImage::ImageRgb8(image::RgbImage::from_fn(3, 2, |x, _| {
            image::Rgb([x as u8, 0, 0])
        }));

        let stats = show(
            &mut display,
            &image,
            DisplayMode::ScrollHorizontal,
            Duration::ZERO,
            DisplayLimit::ScrollCycles {
                cycles: NonZeroU32::new(2).unwrap(),
                min_display_duration: Duration::ZERO,
            },
            &ImageLimits::default(),
            &work,
        )
        .unwrap();

        let offsets: Vec<_> = state
            .borrow()
            .rendered_frames
            .iter()
            .map(|pixels| pixels[0].0)
            .collect();
        assert_eq!(offsets, vec![0, 1, 2, 0, 1, 2]);
        assert_eq!(stats.completed_cycles, 2);
        assert_eq!(stats.prepared_width_px, 3);
    }

    #[test]
    fn minimum_time_can_finish_during_an_additional_cycle() {
        let shutdown = Shutdown::new();
        let work = Work {
            shutdown: &shutdown,
            deadline: Instant::now() + Duration::from_secs(1),
        };
        let state = Rc::new(RefCell::new(State::default()));
        let mut display = FakeDisplay(state.clone());

        let stats = show(
            &mut display,
            &DynamicImage::new_rgb8(1, 2),
            DisplayMode::ScrollHorizontal,
            Duration::ZERO,
            DisplayLimit::ScrollCycles {
                cycles: NonZeroU32::new(1).unwrap(),
                min_display_duration: Duration::from_millis(5),
            },
            &ImageLimits::default(),
            &work,
        )
        .unwrap();

        assert!(stats.completed_cycles >= 1);
        assert!(stats.elapsed >= Duration::from_millis(5));
        assert!(state.borrow().renders > 1);
    }

    #[test]
    fn repeated_renders_do_not_complete_a_cycle_before_the_hold_period() {
        let shutdown = Shutdown::new();
        let work = Work {
            shutdown: &shutdown,
            deadline: Instant::now() + Duration::from_secs(1),
        };
        let state = Rc::new(RefCell::new(State::default()));
        let mut display = FakeDisplay(state.clone());

        let stats = show(
            &mut display,
            &DynamicImage::new_rgb8(1, 2),
            DisplayMode::ScrollHorizontal,
            Duration::from_millis(5),
            DisplayLimit::ScrollCycles {
                cycles: NonZeroU32::new(1).unwrap(),
                min_display_duration: Duration::ZERO,
            },
            &ImageLimits::default(),
            &work,
        )
        .unwrap();

        assert_eq!(stats.completed_cycles, 1);
        assert!(stats.elapsed >= Duration::from_millis(5));
        assert!(state.borrow().renders > 1);
    }

    #[test]
    fn scaling_centers_crop_and_black_padding() {
        let shutdown = Shutdown::new();
        let work = Work {
            shutdown: &shutdown,
            deadline: Instant::now() + Duration::from_secs(1),
        };
        let mut display = FakeDisplay(Rc::new(RefCell::new(State::default())));
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_fn(4, 4, |_, y| {
            image::Rgb([y as u8, 0, 0])
        }));
        let panel = prepare(
            &mut display,
            &img,
            Scale::Width,
            &ImageLimits::default(),
            &work,
        )
        .unwrap();
        assert_eq!(panel.get_pixel(0, 0)[0], 1);
        assert_eq!(panel.get_pixel(0, 1)[0], 2);
        let img =
            DynamicImage::ImageRgb8(image::RgbImage::from_pixel(1, 2, image::Rgb([255, 0, 0])));
        let panel = prepare(
            &mut display,
            &img,
            Scale::Height,
            &ImageLimits::default(),
            &work,
        )
        .unwrap();
        assert_eq!(panel.get_pixel(0, 0)[0], 0);
        assert_eq!(panel.get_pixel(1, 0)[0], 255);
        assert_eq!(panel.get_pixel(2, 0)[0], 0);
    }

    #[test]
    fn animation_render_budget_counts_all_frames() {
        let shutdown = Shutdown::new();
        let work = Work {
            shutdown: &shutdown,
            deadline: Instant::now() + Duration::from_secs(1),
        };
        let state = Rc::new(RefCell::new(State::default()));
        let mut display = FakeDisplay(state.clone());
        let limits = ImageLimits {
            max_render_bytes: 4096,
            ..ImageLimits::default()
        };
        let frames: Vec<_> = (0..200)
            .map(|_| AnimFrame {
                image: DynamicImage::new_rgb8(4, 2),
                delay: Duration::from_millis(10),
            })
            .collect();
        assert!(show_animated(
            &mut display,
            &frames,
            Duration::from_secs(1),
            &limits,
            &work
        )
        .is_err());
        assert_eq!(state.borrow().renders, 0);
    }

    #[cfg(not(feature = "rpi"))]
    #[test]
    fn animation_budget_includes_emulator_screen_at_boundary() {
        // 40 RGB frames plus one preparation canvas and the 10x emulator screen.
        let total = 4 * 2 * (3 * 41 + 400);
        for budget in [total - 1, total] {
            let shutdown = Shutdown::new();
            let work = Work {
                shutdown: &shutdown,
                deadline: Instant::now() + Duration::from_secs(1),
            };
            let state = Rc::new(RefCell::new(State {
                cancel_on_render: Some(shutdown.clone()),
                ..State::default()
            }));
            let mut display = FakeDisplay(state.clone());
            let frames: Vec<_> = (0..40)
                .map(|_| AnimFrame {
                    image: DynamicImage::new_rgb8(4, 2),
                    delay: Duration::from_millis(10),
                })
                .collect();
            let limits = ImageLimits {
                max_render_bytes: budget,
                ..ImageLimits::default()
            };
            let err = show_animated(
                &mut display,
                &frames,
                Duration::from_secs(1),
                &limits,
                &work,
            )
            .unwrap_err();
            if budget < total {
                assert!(err.to_string().contains("MAX_RENDER_BYTES"));
                assert_eq!(state.borrow().renders, 0);
            } else {
                assert!(err.is::<crate::shutdown::Stopped>());
                assert_eq!(state.borrow().renders, 1);
            }
        }
    }

    #[test]
    fn animation_cancellation_stops_during_long_frame() {
        let shutdown = Shutdown::new();
        let work = Work {
            shutdown: &shutdown,
            deadline: Instant::now() + Duration::from_secs(30),
        };
        let state = Rc::new(RefCell::new(State {
            cancel_on_render: Some(shutdown.clone()),
            ..State::default()
        }));
        let mut display = FakeDisplay(state.clone());
        let frames = vec![AnimFrame {
            image: DynamicImage::new_rgb8(4, 2),
            delay: Duration::from_secs(30),
        }];
        let err = show_animated(
            &mut display,
            &frames,
            Duration::from_secs(30),
            &ImageLimits::default(),
            &work,
        )
        .unwrap_err();
        assert!(err.is::<crate::shutdown::Stopped>());
        assert_eq!(state.borrow().renders, 1);
    }
}

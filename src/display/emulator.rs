use anyhow::Result;
use minifb::{Key, Window, WindowOptions};

use crate::config::Config;

use super::LedDisplay;

/// Scale factor: each LED pixel is rendered as SCALE x SCALE screen pixels.
const SCALE: usize = 10;

/// Radius of the circular LED dot within each SCALE x SCALE cell.
/// Pixels outside this radius are rendered black to simulate gaps between LEDs.
const LED_RADIUS: f32 = (SCALE as f32) * 0.45;

pub struct EmulatorDisplay {
    rows: usize,
    cols: usize,
    win_w: usize,
    win_h: usize,
    window: Option<Window>,
}

impl EmulatorDisplay {
    pub fn new(cfg: &Config) -> Self {
        let rows = cfg.panel_rows as usize;
        let cols = cfg.panel_cols as usize;
        Self {
            rows,
            cols,
            win_w: cols * SCALE,
            win_h: rows * SCALE,
            window: None,
        }
    }

    fn window(&mut self) -> Result<&mut Window> {
        if self.window.is_none() {
            let win = Window::new(
                "LED Panel Emulator",
                self.win_w,
                self.win_h,
                WindowOptions {
                    resize: false,
                    ..Default::default()
                },
            )
            .map_err(|e| anyhow::anyhow!("minifb: {e}"))?;
            self.window = Some(win);
        }
        Ok(self.window.as_mut().unwrap())
    }
}

impl LedDisplay for EmulatorDisplay {
    fn rows(&self) -> usize {
        self.rows
    }

    fn cols(&self) -> usize {
        self.cols
    }

    fn render_frame(&mut self, pixels: &[(u8, u8, u8)]) -> Result<()> {
        let win_w = self.win_w;
        let win_h = self.win_h;

        // Build scaled u32 buffer (0x00RRGGBB).
        // Each LED pixel is drawn as a filled circle; pixels outside the circle
        // remain black to simulate the gap between physical LEDs.
        let mut buffer = vec![0u32; win_w * win_h];
        let center = (SCALE as f32 - 1.0) / 2.0;
        for py in 0..self.rows {
            for px in 0..self.cols {
                let (r, g, b) = pixels[py * self.cols + px];
                let color = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
                for sy in 0..SCALE {
                    for sx in 0..SCALE {
                        let dx = sx as f32 - center;
                        let dy = sy as f32 - center;
                        let pixel_color = if dx * dx + dy * dy <= LED_RADIUS * LED_RADIUS {
                            color
                        } else {
                            0 // black gap
                        };
                        buffer[(py * SCALE + sy) * win_w + (px * SCALE + sx)] = pixel_color;
                    }
                }
            }
        }

        let win = self.window()?;
        if win.is_open() && !win.is_key_down(Key::Escape) {
            win.update_with_buffer(&buffer, win_w, win_h)
                .map_err(|e| anyhow::anyhow!("minifb update: {e}"))?;
        }
        std::thread::sleep(super::FRAME_INTERVAL);
        Ok(())
    }

    fn clear(&mut self) -> Result<()> {
        let win_w = self.win_w;
        let win_h = self.win_h;
        let black = vec![0u32; win_w * win_h];
        if let Ok(win) = self.window() {
            let _ = win.update_with_buffer(&black, win_w, win_h);
        }
        Ok(())
    }
}

use anyhow::Result;
use rpi_led_panel::{Canvas, RGBMatrix, RGBMatrixConfig};

use crate::config::Config;

use super::LedDisplay;

/// RPi LED panel backend using the `rpi-led-panel` crate.
///
/// Wraps [`RGBMatrix`] and its double-buffered [`Canvas`].
/// [`render_frame`] writes pixels to the canvas and calls
/// [`RGBMatrix::update_on_vsync`] to swap buffers at the panel's refresh rate.
pub struct RpiDisplay {
    matrix: RGBMatrix,
    canvas: Option<Box<Canvas>>,
    rows: usize,
    cols: usize,
}

impl RpiDisplay {
    pub fn new(cfg: &Config) -> Result<Self> {
        let mut config = RGBMatrixConfig::default();
        config.hardware_mapping = rpi_led_panel::HardwareMapping::regular();
        config.rows = cfg.panel_rows as usize;
        config.cols = cfg.panel_cols as usize;
        config.led_brightness = cfg.panel_brightness;
        config.refresh_rate = cfg.panel_refresh_rate;
        config.slowdown = cfg.panel_slowdown;

        let (matrix, canvas) =
            RGBMatrix::new(config, 0).map_err(|e| anyhow::anyhow!("RGBMatrix init: {e}"))?;

        Ok(Self {
            matrix,
            canvas: Some(canvas),
            rows: cfg.panel_rows as usize,
            cols: cfg.panel_cols as usize,
        })
    }
}

impl LedDisplay for RpiDisplay {
    fn rows(&self) -> usize {
        self.rows
    }

    fn cols(&self) -> usize {
        self.cols
    }

    fn render_frame(&mut self, pixels: &[(u8, u8, u8)]) -> Result<()> {
        let mut canvas = self.canvas.take().expect("canvas missing");

        for (i, &(r, g, b)) in pixels.iter().enumerate() {
            let x = i % self.cols;
            let y = i / self.cols;
            canvas.set_pixel(x, y, r, g, b);
        }

        // update_on_vsync swaps the canvas with the display thread and
        // returns a cleared canvas ready for the next frame.
        self.canvas = Some(self.matrix.update_on_vsync(canvas));
        Ok(())
    }

    fn clear(&mut self) -> Result<()> {
        let mut canvas = self.canvas.take().expect("canvas missing");
        canvas.fill(0, 0, 0);
        self.canvas = Some(self.matrix.update_on_vsync(canvas));
        Ok(())
    }
}

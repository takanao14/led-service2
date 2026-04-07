use anyhow::Result;
use rpi_led_matrix::{LedCanvas, LedColor, LedMatrix, LedMatrixOptions, LedRuntimeOptions};

use crate::config::Config;

use super::LedDisplay;

/// RPi LED panel backend using the `rpi-led-matrix` crate.
pub struct RpiDisplay {
    matrix: LedMatrix,
    canvas: Option<LedCanvas>,
    rows: usize,
    cols: usize,
}

impl RpiDisplay {
    pub fn new(cfg: &Config) -> Result<Self> {
        let mut options = LedMatrixOptions::new();
        options.set_hardware_mapping("regular");
        options.set_rows(cfg.panel_rows);
        options.set_cols(cfg.panel_cols);
        options.set_brightness(cfg.panel_brightness).map_err(|e| anyhow::anyhow!("brightness: {e}"))?;
        if cfg.panel_refresh_rate > 0 {
            options.set_limit_refresh(cfg.panel_refresh_rate as u32);
        }
        options.set_refresh_rate(false);
        options.set_pwm_bits(cfg.panel_pwm_bits as u8).map_err(|e| anyhow::anyhow!("pwm_bits: {e}"))?;
        options.set_pwm_lsb_nanoseconds(cfg.panel_pwm_lsb_nanoseconds);
        options.set_pwm_dither_bits(0);
        // Disable hardware pulsing (PWM/PCM DMA) to prevent USB bus interference on RPi 3.
        // Hardware pulsing improves LED quality but is known to corrupt USB on RPi 3.
        options.set_hardware_pulsing(false);

        let mut rt_options = LedRuntimeOptions::new();
        if let Some(slowdown) = cfg.panel_slowdown {
            rt_options.set_gpio_slowdown(slowdown);
        }

        let matrix = LedMatrix::new(Some(options), Some(rt_options))
            .map_err(|e| anyhow::anyhow!("LedMatrix init: {e}"))?;
        let canvas = matrix.offscreen_canvas();

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
        let mut canvas = self.canvas.take()
            .ok_or_else(|| anyhow::anyhow!("canvas unavailable (lost after a previous panic?)"))?;

        for (i, &(r, g, b)) in pixels.iter().enumerate() {
            let x = (i % self.cols) as i32;
            let y = (i / self.cols) as i32;
            canvas.set(x, y, &LedColor { red: r, green: g, blue: b });
        }

        self.canvas = Some(self.matrix.swap(canvas));
        Ok(())
    }

    fn clear(&mut self) -> Result<()> {
        let mut canvas = self.canvas.take()
            .ok_or_else(|| anyhow::anyhow!("canvas unavailable (lost after a previous panic?)"))?;
        canvas.clear();
        self.canvas = Some(self.matrix.swap(canvas));
        Ok(())
    }
}

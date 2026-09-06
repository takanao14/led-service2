use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::config::{Config, ImageLimits};
use crate::display::{LedDisplay, WindowClosedError};
use crate::shutdown::Shutdown;

#[derive(Default)]
pub struct State {
    pub renders: usize,
    pub clears: usize,
    pub polls: usize,
    pub close_on_poll: bool,
    pub close_on_render: bool,
    pub cancel_on_render: Option<Shutdown>,
}

pub struct FakeDisplay(pub Rc<RefCell<State>>);

impl LedDisplay for FakeDisplay {
    fn rows(&self) -> usize {
        2
    }
    fn cols(&self) -> usize {
        4
    }
    fn render_frame(&mut self, _: &[(u8, u8, u8)]) -> anyhow::Result<()> {
        let mut state = self.0.borrow_mut();
        state.renders += 1;
        if state.close_on_render {
            return Err(WindowClosedError.into());
        }
        if let Some(shutdown) = &state.cancel_on_render {
            shutdown.cancel();
        }
        std::thread::sleep(Duration::from_millis(1));
        Ok(())
    }
    fn poll_events(&mut self) -> anyhow::Result<()> {
        let mut state = self.0.borrow_mut();
        state.polls += 1;
        if state.close_on_poll {
            return Err(WindowClosedError.into());
        }
        Ok(())
    }
    fn clear(&mut self) -> anyhow::Result<()> {
        self.0.borrow_mut().clears += 1;
        Ok(())
    }
}

pub fn config() -> Config {
    Config {
        grpc_addr: "127.0.0.1:0".parse().unwrap(),
        worker_timeout: Duration::from_secs(30),
        panel_rows: 2,
        panel_cols: 4,
        panel_brightness: 50,
        scroll_interval: Duration::from_millis(30),
        jingle_path: None,
        panel_refresh_rate: 120,
        panel_slowdown: None,
        panel_pwm_bits: 11,
        panel_pwm_lsb_nanoseconds: 130,
        eyecatch_path: None,
        eyecatch_duration: Duration::from_secs(3),
        image_limits: ImageLimits::default(),
    }
}

pub fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(width, height)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

pub fn gif(width: u32, height: u32, count: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
        for _ in 0..count {
            encoder
                .encode_frame(image::Frame::new(image::RgbaImage::new(width, height)))
                .unwrap();
        }
    }
    bytes
}

use std::time::Duration;

/// Runtime configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// gRPC listen address (env: `GRPC_ADDR`, default: `0.0.0.0:50051`)
    pub grpc_addr: String,
    /// Maximum time allowed per display request (env: `WORKER_TIMEOUT`, default: `30s`)
    pub worker_timeout: Duration,
    /// Number of LED panel rows (env: `PANEL_ROWS`, default: `32`)
    pub panel_rows: u32,
    /// Number of LED panel columns (env: `PANEL_COLS`, default: `64`)
    pub panel_cols: u32,
    /// LED panel brightness, 0–100 (env: `PANEL_BRIGHTNESS`, default: `50`)
    pub panel_brightness: u8,
    /// Scroll speed: time between advancing 1 pixel (env: `SCROLL_INTERVAL_MS`, default: `30`)
    pub scroll_interval: Duration,
    /// Optional WAV file to play each time an image is displayed (env: `JINGLE_PATH`)
    pub jingle_path: Option<String>,
    /// Panel refresh rate in Hz (env: `PANEL_REFRESH_RATE`, default: `120`)
    pub panel_refresh_rate: usize,
    /// GPIO slowdown factor for RPi (env: `PANEL_SLOWDOWN`, default: unset)
    pub panel_slowdown: Option<u32>,
    /// Optional GIF file to show before the main image (env: `EYECATCH_PATH`)
    pub eyecatch_path: Option<String>,
    /// How long to display the eye-catch GIF (env: `EYECATCH_DURATION_MS`, default: `3000`)
    pub eyecatch_duration: Duration,
}

impl Config {
    /// Load configuration from environment variables, falling back to defaults.
    ///
    /// # Errors
    /// Returns an error if `WORKER_TIMEOUT` is set but cannot be parsed as a duration.
    pub fn from_env() -> anyhow::Result<Self> {
        let grpc_addr = std::env::var("GRPC_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:50051".to_string());

        let worker_timeout = match std::env::var("WORKER_TIMEOUT") {
            Ok(v) => humantime::parse_duration(&v)
                .map_err(|e| anyhow::anyhow!("invalid WORKER_TIMEOUT: {e}"))?,
            Err(_) => Duration::from_secs(30),
        };

        let panel_rows = std::env::var("PANEL_ROWS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(32);

        let panel_cols = std::env::var("PANEL_COLS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(64);

        let panel_brightness = std::env::var("PANEL_BRIGHTNESS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50);

        let scroll_interval_ms: u64 = std::env::var("SCROLL_INTERVAL_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);

        let panel_refresh_rate: usize = std::env::var("PANEL_REFRESH_RATE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(120);

        let panel_slowdown: Option<u32> = std::env::var("PANEL_SLOWDOWN")
            .ok()
            .and_then(|v| v.parse().ok());

        let jingle_path = std::env::var("JINGLE_PATH").ok();
        let eyecatch_path = std::env::var("EYECATCH_PATH").ok();
        let eyecatch_duration_ms: u64 = std::env::var("EYECATCH_DURATION_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3000);

        Ok(Self {
            grpc_addr,
            worker_timeout,
            panel_rows,
            panel_cols,
            panel_brightness,
            scroll_interval: Duration::from_millis(scroll_interval_ms),
            panel_refresh_rate,
            panel_slowdown,
            jingle_path,
            eyecatch_path,
            eyecatch_duration: Duration::from_millis(eyecatch_duration_ms),
        })
    }
}

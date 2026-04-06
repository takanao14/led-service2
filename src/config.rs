use std::net::SocketAddr;
use std::time::Duration;

/// Runtime configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// gRPC listen address (env: `GRPC_ADDR`, default: `0.0.0.0:50051`)
    pub grpc_addr: SocketAddr,
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
        let grpc_addr: SocketAddr = std::env::var("GRPC_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:50051".to_string())
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid GRPC_ADDR: {e}"))?;

        let worker_timeout = match std::env::var("WORKER_TIMEOUT") {
            Ok(v) => humantime::parse_duration(&v)
                .map_err(|e| anyhow::anyhow!("invalid WORKER_TIMEOUT: {e}"))?,
            Err(_) => Duration::from_secs(30),
        };

        let panel_rows = match std::env::var("PANEL_ROWS") {
            Ok(v) => v.parse::<u32>().unwrap_or_else(|e| {
                tracing::warn!(value = %v, error = %e, "invalid PANEL_ROWS, using default 32");
                32
            }),
            Err(_) => 32,
        };

        let panel_cols = match std::env::var("PANEL_COLS") {
            Ok(v) => v.parse::<u32>().unwrap_or_else(|e| {
                tracing::warn!(value = %v, error = %e, "invalid PANEL_COLS, using default 64");
                64
            }),
            Err(_) => 64,
        };

        let panel_brightness = match std::env::var("PANEL_BRIGHTNESS") {
            Ok(v) => v.parse::<u8>().unwrap_or_else(|e| {
                tracing::warn!(value = %v, error = %e, "invalid PANEL_BRIGHTNESS, using default 50");
                50
            }),
            Err(_) => 50,
        };

        let scroll_interval_ms: u64 = match std::env::var("SCROLL_INTERVAL_MS") {
            Ok(v) => v.parse::<u64>().unwrap_or_else(|e| {
                tracing::warn!(value = %v, error = %e, "invalid SCROLL_INTERVAL_MS, using default 30");
                30
            }),
            Err(_) => 30,
        };

        let panel_refresh_rate: usize = match std::env::var("PANEL_REFRESH_RATE") {
            Ok(v) => v.parse::<usize>().unwrap_or_else(|e| {
                tracing::warn!(value = %v, error = %e, "invalid PANEL_REFRESH_RATE, using default 120");
                120
            }),
            Err(_) => 120,
        };

        let panel_slowdown: Option<u32> = match std::env::var("PANEL_SLOWDOWN") {
            Ok(v) => match v.parse::<u32>() {
                Ok(n) => Some(n),
                Err(e) => {
                    tracing::warn!(value = %v, error = %e, "invalid PANEL_SLOWDOWN, ignoring");
                    None
                }
            },
            Err(_) => None,
        };

        let jingle_path = std::env::var("JINGLE_PATH").ok();
        let eyecatch_path = std::env::var("EYECATCH_PATH").ok();
        let eyecatch_duration_ms: u64 = match std::env::var("EYECATCH_DURATION_MS") {
            Ok(v) => v.parse::<u64>().unwrap_or_else(|e| {
                tracing::warn!(value = %v, error = %e, "invalid EYECATCH_DURATION_MS, using default 5000");
                5000
            }),
            Err(_) => 5000,
        };

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

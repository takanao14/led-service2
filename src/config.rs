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
    /// PWM bits (env: `PANEL_PWM_BITS`, default: 11)
    pub panel_pwm_bits: u32,
    /// PWM LSB nanoseconds (env: `PANEL_PWM_LSB_NANOSECONDS`, default: 130)
    pub panel_pwm_lsb_nanoseconds: u32,
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

        let panel_rows      = env_parse::<u32>("PANEL_ROWS", 32);
        let panel_cols      = env_parse::<u32>("PANEL_COLS", 64);

        // Brightness is 0–100; clamp silently after parsing.
        let panel_brightness = {
            let raw = env_parse::<u8>("PANEL_BRIGHTNESS", 50);
            if raw > 100 {
                tracing::warn!(value = raw, "PANEL_BRIGHTNESS exceeds 100, clamping to 100");
                100
            } else {
                raw
            }
        };

        let scroll_interval_ms      = env_parse::<u64>("SCROLL_INTERVAL_MS", 30);
        let panel_refresh_rate      = env_parse::<usize>("PANEL_REFRESH_RATE", 120);
        let panel_slowdown          = env_parse_opt::<u32>("PANEL_SLOWDOWN");
        let panel_pwm_bits          = env_parse::<u32>("PANEL_PWM_BITS", 11);
        let panel_pwm_lsb_nanoseconds = env_parse::<u32>("PANEL_PWM_LSB_NANOSECONDS", 130);
        let jingle_path             = std::env::var("JINGLE_PATH").ok();
        let eyecatch_path           = std::env::var("EYECATCH_PATH").ok();
        // Default: 3000 ms (matches the field-level doc comment).
        let eyecatch_duration_ms    = env_parse::<u64>("EYECATCH_DURATION_MS", 3000);

        Ok(Self {
            grpc_addr,
            worker_timeout,
            panel_rows,
            panel_cols,
            panel_brightness,
            scroll_interval: Duration::from_millis(scroll_interval_ms),
            panel_refresh_rate,
            panel_slowdown,
            panel_pwm_bits,
            panel_pwm_lsb_nanoseconds,
            jingle_path,
            eyecatch_path,
            eyecatch_duration: Duration::from_millis(eyecatch_duration_ms),
        })
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Parse an environment variable as `T`, falling back to `default` and logging
/// a warning when the value is present but cannot be parsed.
fn env_parse<T>(key: &str, default: T) -> T
where
    T: std::str::FromStr + std::fmt::Display + Copy,
    T::Err: std::fmt::Display,
{
    match std::env::var(key) {
        Ok(v) => v.parse::<T>().unwrap_or_else(|e| {
            tracing::warn!(value = %v, error = %e, key, "invalid env var, using default {default}");
            default
        }),
        Err(_) => default,
    }
}

/// Parse an optional environment variable as `T`.
/// Returns `None` if the variable is unset; logs a warning and returns `None`
/// if the value is present but cannot be parsed.
fn env_parse_opt<T>(key: &str) -> Option<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match std::env::var(key) {
        Ok(v) => match v.parse::<T>() {
            Ok(n) => Some(n),
            Err(e) => {
                tracing::warn!(value = %v, error = %e, key, "invalid env var, ignoring");
                None
            }
        },
        Err(_) => None,
    }
}

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use std::time::Duration;

use led_service2::proto;
use proto::image_service_client::ImageServiceClient;
use proto::{DisplayMode, ImageData, SendImageRequest};

#[derive(Parser, Debug)]
#[command(about = "LED service client — send an image to the LED panel server")]
struct Args {
    /// gRPC server address
    #[arg(long, default_value = "http://localhost:50051")]
    addr: String,

    /// Image file to send (.png, .jpg, .gif, .ppm)
    #[arg(long)]
    file: String,

    /// MIME type (auto-detected from file extension if omitted)
    #[arg(long)]
    mime: Option<String>,

    /// Display duration in seconds (must be ≥ 1)
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(i32).range(1..))]
    duration: i32,

    /// Display mode (default: inferred from file type — PPM scrolls, others are static)
    #[arg(long, value_enum)]
    display_mode: Option<DisplayModeArg>,

    /// Minimum horizontal scroll cycles (uses --duration as an older-server fallback)
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    scroll_cycles: Option<u32>,

    /// Minimum main display time for cycle-based scrolling
    #[arg(long, default_value_t = 0)]
    min_display_seconds: u32,
}

#[derive(Debug, Clone, ValueEnum)]
enum DisplayModeArg {
    /// Show image statically.
    Static,
    /// Scroll image horizontally.
    Scroll,
}

fn validate_scroll_options(args: &Args, mime_type: &str) -> Result<()> {
    if args.min_display_seconds > 0 && args.scroll_cycles.is_none() {
        anyhow::bail!("--min-display-seconds requires --scroll-cycles");
    }
    if args.scroll_cycles.is_some() && mime_type.eq_ignore_ascii_case("image/gif") {
        anyhow::bail!("--scroll-cycles cannot be used with GIF images");
    }
    if args.scroll_cycles.is_some() && matches!(args.display_mode, Some(DisplayModeArg::Static)) {
        anyhow::bail!("--scroll-cycles cannot be used with --display-mode static");
    }
    Ok(())
}

fn detect_mime(path: &str) -> Option<&'static str> {
    let lower = path.to_lowercase();
    if lower.ends_with(".ppm") || lower.ends_with(".pnm") || lower.ends_with(".pgm") {
        Some("image/x-portable-pixmap")
    } else if lower.ends_with(".gif") {
        Some("image/gif")
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Some("image/jpeg")
    } else if lower.ends_with(".png") {
        Some("image/png")
    } else {
        None
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let image_data =
        std::fs::read(&args.file).with_context(|| format!("failed to read file: {}", args.file))?;

    let mime_type = args
        .mime
        .clone()
        .unwrap_or_else(|| match detect_mime(&args.file) {
            Some(m) => m.to_string(),
            None => {
                eprintln!(
                    "warning: unknown file extension for '{}', assuming image/png",
                    args.file
                );
                "image/png".to_string()
            }
        });

    validate_scroll_options(&args, &mime_type)?;

    let display_mode = match args.display_mode {
        Some(DisplayModeArg::Static) => DisplayMode::Static as i32,
        Some(DisplayModeArg::Scroll) => DisplayMode::Scroll as i32,
        None if args.scroll_cycles.is_some() => DisplayMode::Scroll as i32,
        None => DisplayMode::Unspecified as i32,
    };

    // This bounds RPC admission, not display completion. Keep the duration-based
    // compatibility margin when cycle options are sent to an older server.
    let request_timeout = Duration::from_secs(args.duration as u64 + 10);
    let endpoint = tonic::transport::Endpoint::from_shared(args.addr.clone())
        .with_context(|| format!("invalid address: {}", args.addr))?
        .connect_timeout(Duration::from_secs(5))
        .timeout(request_timeout);

    let mut client = ImageServiceClient::connect(endpoint)
        .await
        .with_context(|| format!("failed to connect to {}", args.addr))?;

    let request = SendImageRequest {
        image: Some(ImageData {
            image_data,
            mime_type,
        }),
        duration_seconds: args.duration,
        display_mode,
        scroll_cycles: args.scroll_cycles.unwrap_or(0),
        min_display_seconds: args.min_display_seconds,
    };

    let response = client.send_image(request).await?.into_inner();

    if response.success {
        println!("success: {}", response.message);
    } else {
        eprintln!("error: {}", response.message);
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(extra: &[&str]) -> Args {
        let mut values = vec!["led-client", "--file", "image.png"];
        values.extend_from_slice(extra);
        Args::try_parse_from(values).unwrap()
    }

    #[test]
    fn validates_scroll_option_combinations() {
        assert!(validate_scroll_options(&args(&[]), "image/png").is_ok());
        assert!(validate_scroll_options(
            &args(&["--scroll-cycles", "2", "--min-display-seconds", "5"]),
            "image/png"
        )
        .is_ok());
        assert!(
            validate_scroll_options(&args(&["--min-display-seconds", "5"]), "image/png").is_err()
        );
        assert!(validate_scroll_options(&args(&["--scroll-cycles", "2"]), "image/gif").is_err());
        assert!(validate_scroll_options(
            &args(&["--scroll-cycles", "2", "--display-mode", "static"]),
            "image/png"
        )
        .is_err());
    }

    #[test]
    fn rejects_zero_scroll_cycles() {
        assert!(Args::try_parse_from([
            "led-client",
            "--file",
            "image.png",
            "--scroll-cycles",
            "0"
        ])
        .is_err());
    }
}

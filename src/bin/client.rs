use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};

/// Protocol Buffers generated code for `image.v1`.
pub mod proto {
    tonic::include_proto!("image.v1");
}

use proto::image_service_client::ImageServiceClient;
use proto::{DisplayMode, ImageData, SendImageRequest};

/// Command-line arguments for `led-client`.
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

    /// Display duration in seconds
    #[arg(long, default_value_t = 10)]
    duration: i32,

    /// Display mode (default: inferred from file type — PPM scrolls, others are static)
    #[arg(long, value_enum)]
    display_mode: Option<DisplayModeArg>,
}

/// Display mode selectable from the command line.
#[derive(Debug, Clone, ValueEnum)]
enum DisplayModeArg {
    /// Show image statically.
    Static,
    /// Scroll image horizontally.
    Scroll,
}

/// Infer the MIME type from the file extension.
///
/// Falls back to `image/png` for unknown extensions.
fn detect_mime(path: &str) -> &'static str {
    let lower = path.to_lowercase();
    if lower.ends_with(".ppm") || lower.ends_with(".pnm") || lower.ends_with(".pgm") {
        "image/x-portable-pixmap"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else {
        "image/png"
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let image_data = std::fs::read(&args.file)
        .with_context(|| format!("failed to read file: {}", args.file))?;

    let mime_type = args
        .mime
        .unwrap_or_else(|| detect_mime(&args.file).to_string());

    let display_mode = match args.display_mode {
        Some(DisplayModeArg::Static) => DisplayMode::Static as i32,
        Some(DisplayModeArg::Scroll) => DisplayMode::Scroll as i32,
        None => DisplayMode::Unspecified as i32,
    };

    let mut client = ImageServiceClient::connect(args.addr.clone())
        .await
        .with_context(|| format!("failed to connect to {}", args.addr))?;

    let request = SendImageRequest {
        image: Some(ImageData {
            image_data,
            mime_type,
        }),
        duration_seconds: args.duration,
        display_mode,
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

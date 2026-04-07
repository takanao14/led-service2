mod config;
mod display;
mod service;
mod worker;

// Re-export the proto module from the library crate so that submodules can
// continue to use `crate::proto::...` without any changes.
pub use led_service2::proto;

use proto::image_service_server::ImageServiceServer;
use service::LedImageService;
use tonic::transport::Server;
use worker::DisplayRequest;

/// Entry point.
///
/// # Threading model
/// - **Main thread**: runs the display loop (required by minifb/Cocoa on macOS).
/// - **Background thread**: runs the tokio runtime hosting the gRPC server.
/// - A bounded channel (capacity 10) connects the gRPC service to the display loop.
///
/// # Logging
/// Set `RUST_LOG` to control log level (e.g. `RUST_LOG=debug`).
/// Set `LOG_FORMAT=json` to switch to structured JSON output (recommended for production).
fn main() -> anyhow::Result<()> {
    let env_filter = tracing_subscriber::EnvFilter::from_default_env()
        .add_directive("led_service2=info".parse()?);

    if std::env::var("LOG_FORMAT").as_deref() == Ok("json") {
        tracing_subscriber::fmt().json().with_env_filter(env_filter).init();
    } else {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    };

    let cfg = config::Config::from_env()?;
    tracing::info!(
        grpc_addr = %cfg.grpc_addr,
        worker_timeout = ?cfg.worker_timeout,
        "starting led-service2"
    );

    // Bounded channel connecting the gRPC handler to the display worker.
    let (tx, rx) = std::sync::mpsc::sync_channel::<DisplayRequest>(10);

    // gRPC server runs in a background thread so the main thread stays free
    // for the display loop.
    let cfg_grpc = cfg.clone();
    let grpc_handle = std::thread::spawn(move || {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime")
            .block_on(async move {
                let addr = cfg_grpc.grpc_addr;
                let svc = LedImageService::new(tx);

                tracing::info!(%addr, "gRPC server listening");
                if let Err(e) = Server::builder()
                    .add_service(ImageServiceServer::new(svc))
                    .serve_with_shutdown(addr, shutdown_signal())
                    .await
                {
                    tracing::error!(error = %e, "gRPC server error");
                }
                tracing::info!("gRPC server stopped");
            });
    });

    // Display loop must run on the main thread (minifb requires Cocoa on macOS).
    let display = display::create(&cfg)?;
    worker::run_loop(
        display,
        rx,
        cfg.worker_timeout,
        cfg.scroll_interval,
        cfg.jingle_path,
        cfg.eyecatch_path,
        cfg.eyecatch_duration,
    );

    if let Err(e) = grpc_handle.join() {
        tracing::error!("gRPC server thread panicked: {:?}", e);
    }

    Ok(())
}

/// Wait for SIGINT (Ctrl+C) or SIGTERM and return.
async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received");
}

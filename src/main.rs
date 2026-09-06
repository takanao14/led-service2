mod config;
mod display;
mod service;
mod shutdown;
mod worker;

// Re-export the proto module from the library crate so that submodules can
// continue to use `crate::proto::...` without any changes.
pub use led_service2::proto;

use anyhow::Context;
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
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(env_filter)
            .init();
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
    let shutdown = shutdown::Shutdown::new();
    let grpc_shutdown = shutdown.clone();
    let grpc_handle = std::thread::spawn(move || -> anyhow::Result<()> {
        let _guard = grpc_shutdown.guard();
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("failed to build tokio runtime")?
            .block_on(async move {
                let addr = cfg_grpc.grpc_addr;
                let svc = LedImageService::new(tx, grpc_shutdown.clone());

                tracing::info!(%addr, "starting gRPC server");
                let server = Server::builder()
                    .add_service(ImageServiceServer::new(svc))
                    .serve_with_shutdown(addr, async {
                        tokio::select! {
                            _ = shutdown_signal() => grpc_shutdown.cancel(),
                            _ = grpc_shutdown.cancelled() => {},
                        }
                    });
                tokio::pin!(server);
                let result = tokio::select! {
                    result = &mut server => result,
                    _ = grpc_shutdown.cancelled() => {
                        match tokio::time::timeout(std::time::Duration::from_secs(2), &mut server).await {
                            Ok(result) => result,
                            Err(_) => {
                                tracing::warn!("gRPC graceful shutdown exceeded 2 seconds; closing connections");
                                Ok(())
                            }
                        }
                    }
                };
                result.with_context(|| format!("gRPC server failed at {addr}"))?;
                tracing::info!("gRPC server stopped");
                Ok(())
            })
    });

    // Display loop must run on the main thread (minifb requires Cocoa on macOS).
    let worker_result =
        display::create(&cfg).and_then(|display| worker::run_loop(display, rx, &cfg, &shutdown));
    shutdown.cancel();

    grpc_handle
        .join()
        .map_err(|_| anyhow::anyhow!("gRPC server thread panicked"))??;

    worker_result
}

/// Wait for SIGINT (Ctrl+C) or SIGTERM and return.
async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
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

#[cfg(test)]
mod test_support;

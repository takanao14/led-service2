use crate::shutdown::Shutdown;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::Duration;

use tonic::{Request, Response, Status};

use crate::proto::image_service_server::ImageService;
use crate::proto::{DisplayMode, SendImageRequest, SendImageResponse};
use crate::worker::DisplayRequest;

/// gRPC service implementation for `image.v1.ImageService`.
///
/// Incoming requests are validated and immediately enqueued for display.
/// The response is returned as soon as the request is queued; actual display
/// happens asynchronously in the worker thread.
pub struct LedImageService {
    /// Sender side of the bounded display queue (capacity 10).
    queue_tx: SyncSender<DisplayRequest>,
    shutdown: Shutdown,
}

impl LedImageService {
    /// Create a new service that enqueues display requests onto `queue_tx`.
    pub fn new(queue_tx: SyncSender<DisplayRequest>, shutdown: Shutdown) -> Self {
        Self { queue_tx, shutdown }
    }
}

#[tonic::async_trait]
impl ImageService for LedImageService {
    /// Validate and enqueue an image for display.
    ///
    /// # Errors
    /// - `InvalidArgument` if `image` is missing, `image_data` is empty, or `duration_seconds ≤ 0`.
    /// - `ResourceExhausted` if the display queue is full (capacity: 10).
    async fn send_image(
        &self,
        request: Request<SendImageRequest>,
    ) -> Result<Response<SendImageResponse>, Status> {
        if self.shutdown.is_cancelled() {
            return Err(Status::unavailable("display service is shutting down"));
        }
        let req = request.into_inner();

        let image = req
            .image
            .ok_or_else(|| Status::invalid_argument("image is required"))?;
        if image.image_data.is_empty() {
            return Err(Status::invalid_argument("image_data is empty"));
        }
        if req.duration_seconds <= 0 {
            return Err(Status::invalid_argument("duration_seconds must be > 0"));
        }

        // Non-blocking send: returns ResourceExhausted if the queue is full.
        let display_req = DisplayRequest {
            image_data: image.image_data,
            mime_type: image.mime_type,
            duration: Duration::from_secs(req.duration_seconds as u64),
            display_mode: DisplayMode::try_from(req.display_mode)
                .unwrap_or(DisplayMode::Unspecified),
        };

        self.queue_tx.try_send(display_req).map_err(|e| match e {
            TrySendError::Full(_) => Status::resource_exhausted("display queue is full"),
            TrySendError::Disconnected(_) => Status::unavailable("display worker has stopped"),
        })?;

        tracing::info!(duration_seconds = req.duration_seconds, "request queued");

        Ok(Response::new(SendImageResponse {
            success: true,
            message: "queued".to_string(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::ImageData;

    fn request() -> Request<SendImageRequest> {
        Request::new(SendImageRequest {
            image: Some(ImageData {
                image_data: vec![1],
                mime_type: "image/png".into(),
            }),
            duration_seconds: 1,
            display_mode: 0,
        })
    }

    #[tokio::test]
    async fn distinguishes_full_disconnected_and_stopping() {
        let shutdown = Shutdown::new();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let service = LedImageService::new(tx, shutdown.clone());
        service.send_image(request()).await.unwrap();
        assert_eq!(
            service.send_image(request()).await.unwrap_err().code(),
            tonic::Code::ResourceExhausted
        );
        drop(rx);
        assert_eq!(
            service.send_image(request()).await.unwrap_err().code(),
            tonic::Code::Unavailable
        );
        shutdown.cancel();
        assert_eq!(
            service.send_image(request()).await.unwrap_err().code(),
            tonic::Code::Unavailable
        );
    }
}

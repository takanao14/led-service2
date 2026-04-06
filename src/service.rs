use std::sync::mpsc::SyncSender;
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
}

impl LedImageService {
    /// Create a new service that enqueues display requests onto `queue_tx`.
    pub fn new(queue_tx: SyncSender<DisplayRequest>) -> Self {
        Self { queue_tx }
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

        self.queue_tx.try_send(display_req).map_err(|_| {
            Status::resource_exhausted("display queue is full")
        })?;

        tracing::info!(duration_seconds = req.duration_seconds, "request queued");

        Ok(Response::new(SendImageResponse {
            success: true,
            message: "queued".to_string(),
        }))
    }
}

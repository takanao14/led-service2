use crate::shutdown::Shutdown;
use std::num::NonZeroU32;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::Duration;

use tonic::{Request, Response, Status};

use crate::display::{DisplayLimit, DisplayMode as EffectiveDisplayMode};
use crate::proto::image_service_server::ImageService;
use crate::proto::{DisplayMode, SendImageRequest, SendImageResponse};
use crate::worker::DisplayRequest;

/// Validates requests and acknowledges queue admission before display begins.
pub struct LedImageService {
    queue_tx: SyncSender<DisplayRequest>,
    shutdown: Shutdown,
}

impl LedImageService {
    pub fn new(queue_tx: SyncSender<DisplayRequest>, shutdown: Shutdown) -> Self {
        Self { queue_tx, shutdown }
    }
}

#[tonic::async_trait]
impl ImageService for LedImageService {
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
        let limit = if let Some(cycles) = NonZeroU32::new(req.scroll_cycles) {
            if req.duration_seconds < 0 {
                return Err(Status::invalid_argument(
                    "duration_seconds must be >= 0 when scroll_cycles is set",
                ));
            }
            let proto_mode = DisplayMode::try_from(req.display_mode).map_err(|_| {
                Status::invalid_argument("unknown display_mode for cycle-based scrolling")
            })?;
            if image.mime_type.eq_ignore_ascii_case("image/gif") {
                return Err(Status::invalid_argument(
                    "scroll_cycles cannot be used with GIF images",
                ));
            }
            if crate::worker::resolve_display_mode(proto_mode, &image.mime_type)
                != EffectiveDisplayMode::ScrollHorizontal
            {
                return Err(Status::invalid_argument(
                    "scroll_cycles requires scroll display mode",
                ));
            }
            DisplayLimit::ScrollCycles {
                cycles,
                min_display_duration: Duration::from_secs(u64::from(req.min_display_seconds)),
            }
        } else {
            if req.min_display_seconds > 0 {
                return Err(Status::invalid_argument(
                    "min_display_seconds requires scroll_cycles",
                ));
            }
            if req.duration_seconds <= 0 {
                return Err(Status::invalid_argument("duration_seconds must be > 0"));
            }
            DisplayLimit::Duration(Duration::from_secs(req.duration_seconds as u64))
        };

        let display_mode = if req.scroll_cycles > 0 {
            DisplayMode::try_from(req.display_mode).expect("cycle mode was validated")
        } else {
            DisplayMode::try_from(req.display_mode).unwrap_or(DisplayMode::Unspecified)
        };

        let display_req = DisplayRequest {
            image_data: image.image_data,
            mime_type: image.mime_type,
            limit,
            display_mode,
        };

        // Rejections are otherwise invisible to operators: the client sees a status
        // code, and the availability probe keeps reporting a healthy service.
        self.queue_tx.try_send(display_req).map_err(|e| match e {
            TrySendError::Full(_) => {
                tracing::warn!("display queue is full, rejecting request");
                Status::resource_exhausted("display queue is full")
            }
            TrySendError::Disconnected(_) => {
                tracing::error!("display worker has stopped, rejecting request");
                Status::unavailable("display worker has stopped")
            }
        })?;

        tracing::info!(
            duration_seconds = req.duration_seconds,
            scroll_cycles = req.scroll_cycles,
            min_display_seconds = req.min_display_seconds,
            "request queued"
        );

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
            scroll_cycles: 0,
            min_display_seconds: 0,
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

    async fn reject(req: SendImageRequest) {
        let shutdown = Shutdown::new();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let service = LedImageService::new(tx, shutdown);
        let err = service.send_image(Request::new(req)).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(
            rx.try_recv().is_err(),
            "rejected request consumed queue space"
        );
    }

    #[tokio::test]
    async fn validates_cycle_request_contract() {
        let mut req = request().into_inner();
        req.duration_seconds = 0;
        reject(req.clone()).await;

        req.scroll_cycles = 1;
        req.display_mode = DisplayMode::Static as i32;
        reject(req.clone()).await;

        req.display_mode = 99;
        reject(req.clone()).await;

        req.display_mode = DisplayMode::Scroll as i32;
        req.duration_seconds = -1;
        reject(req.clone()).await;

        req.duration_seconds = 0;
        req.image.as_mut().unwrap().mime_type = "image/gif".into();
        reject(req).await;

        let mut req = request().into_inner();
        req.min_display_seconds = 1;
        reject(req).await;
    }

    #[tokio::test]
    async fn accepts_cycles_with_zero_duration_and_ppm_inference() {
        let shutdown = Shutdown::new();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let service = LedImageService::new(tx, shutdown);
        let mut req = request().into_inner();
        req.duration_seconds = 0;
        req.scroll_cycles = 2;
        req.min_display_seconds = 5;
        req.image.as_mut().unwrap().mime_type = "image/x-portable-pixmap".into();

        service.send_image(Request::new(req)).await.unwrap();
        let queued = rx.try_recv().unwrap();
        assert!(matches!(
            queued.limit,
            DisplayLimit::ScrollCycles { cycles, min_display_duration }
                if cycles.get() == 2 && min_display_duration == Duration::from_secs(5)
        ));
    }
}

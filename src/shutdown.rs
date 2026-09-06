use tokio::sync::watch;

/// Shared cancellation for the async server and the main-thread display loop.
#[derive(Clone)]
pub struct Shutdown(watch::Sender<bool>);

impl Shutdown {
    pub fn new() -> Self {
        Self(watch::channel(false).0)
    }

    pub fn cancel(&self) {
        self.0.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }

    pub async fn cancelled(&self) {
        let mut rx = self.0.subscribe();
        while !*rx.borrow_and_update() {
            if rx.changed().await.is_err() {
                break;
            }
        }
    }

    pub fn guard(&self) -> CancelOnDrop {
        CancelOnDrop(self.clone())
    }
}

pub struct CancelOnDrop(Shutdown);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_before_subscribing_is_not_lost() {
        let shutdown = Shutdown::new();
        shutdown.cancel();
        tokio::time::timeout(std::time::Duration::from_secs(1), shutdown.cancelled())
            .await
            .unwrap();
    }

    #[test]
    fn guard_cancels_on_exit() {
        let shutdown = Shutdown::new();
        drop(shutdown.guard());
        assert!(shutdown.is_cancelled());
    }
}

/// Cooperative cancellation and deadline checks between bounded units of work.
pub struct Work<'a> {
    pub shutdown: &'a Shutdown,
    pub deadline: std::time::Instant,
}

impl Work<'_> {
    pub fn check(&self) -> anyhow::Result<()> {
        if self.shutdown.is_cancelled() {
            return Err(Stopped::Shutdown.into());
        }
        if std::time::Instant::now() >= self.deadline {
            return Err(Stopped::Deadline.into());
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Stopped {
    #[error("shutdown requested")]
    Shutdown,
    #[error("request deadline exceeded")]
    Deadline,
}

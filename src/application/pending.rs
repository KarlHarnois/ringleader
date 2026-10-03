use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::thread;
use std::time::Duration;

use super::service_error::ServiceError;

pub struct Pending<T> {
    receiver: Receiver<Result<T, ServiceError>>,
}

impl<T: Send + 'static> Pending<T> {
    const POLL_INTERVAL: Duration = Duration::from_millis(100);

    pub fn start(work: impl FnOnce() -> Result<T, ServiceError> + Send + 'static) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(work());
        });
        Self { receiver }
    }

    pub fn poll(&self) -> Option<Result<T, ServiceError>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(Self::worker_failure())),
        }
    }

    pub fn wait_unless(&self, stop: impl Fn() -> bool) -> Option<Result<T, ServiceError>> {
        loop {
            match self.receiver.recv_timeout(Self::POLL_INTERVAL) {
                Ok(result) => return Some(result),
                Err(RecvTimeoutError::Disconnected) => return Some(Err(Self::worker_failure())),
                Err(RecvTimeoutError::Timeout) if stop() => return None,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    fn worker_failure() -> ServiceError {
        ServiceError::Failed("the worker ended without a result".to_string())
    }
}

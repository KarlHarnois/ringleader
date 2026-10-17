use thiserror::Error;

pub trait WebBrowser: Send + Sync {
    fn open(&self, url: &str) -> Result<(), WebBrowserError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WebBrowserError {
    #[error("no graphical display is available, {reason}")]
    NoDisplay { reason: String },
    #[error("no web browser to open, {reason}")]
    NoBrowser { reason: String },
    #[error("cannot start the web browser, {reason}")]
    StartFailed { reason: String },
}

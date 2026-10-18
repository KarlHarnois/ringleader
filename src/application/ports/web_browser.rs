use thiserror::Error;

pub trait WebBrowser: Send + Sync {
    fn open(&self, url: &str) -> Result<(), WebBrowserError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WebBrowserError {
    #[error("no graphical display is available")]
    NoDisplay,
    #[error("no web browser is available")]
    NoBrowser,
    #[error("cannot start the web browser")]
    StartFailed,
}

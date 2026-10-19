use std::sync::Mutex;

use crate::application::ports::{WebBrowser, WebBrowserError};

#[derive(Default)]
pub struct FakeWebBrowser {
    opened: Mutex<Vec<String>>,
    failure: Option<WebBrowserError>,
}

impl FakeWebBrowser {
    pub fn failing(error: WebBrowserError) -> Self {
        Self {
            failure: Some(error),
            ..Self::default()
        }
    }

    pub fn opened(&self) -> Vec<String> {
        self.opened.lock().unwrap().clone()
    }
}

impl WebBrowser for FakeWebBrowser {
    fn open(&self, url: &str) -> Result<(), WebBrowserError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        self.opened.lock().unwrap().push(url.to_string());
        Ok(())
    }
}

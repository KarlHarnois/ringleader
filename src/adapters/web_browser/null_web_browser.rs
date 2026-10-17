use crate::application::ports::{WebBrowser, WebBrowserError};

pub struct NullWebBrowser {
    missing: WebBrowserError,
}

impl NullWebBrowser {
    pub fn new(missing: WebBrowserError) -> Self {
        Self { missing }
    }
}

impl WebBrowser for NullWebBrowser {
    fn open(&self, _url: &str) -> Result<(), WebBrowserError> {
        Err(self.missing.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_url_fails_with_what_the_host_lacks() {
        let missing = WebBrowserError::NoBrowser {
            reason: "xdg-open is not on PATH".to_string(),
        };
        let browser = NullWebBrowser::new(missing.clone());

        assert_eq!(
            browser.open("https://github.example.com/example-org/example-project/pull/142"),
            Err(missing)
        );
    }
}

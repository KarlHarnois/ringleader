use crate::application::ports::{WebBrowser, WebBrowserError};

pub struct NullWebBrowser {
    missing: WebBrowserError,
    reason: String,
}

impl NullWebBrowser {
    pub fn new(missing: WebBrowserError, reason: &str) -> Self {
        Self {
            missing,
            reason: reason.to_string(),
        }
    }
}

impl WebBrowser for NullWebBrowser {
    fn open(&self, _url: &str) -> Result<(), WebBrowserError> {
        log::warn!("cannot open a web browser: {}", self.reason);
        Err(self.missing.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::LogCapture;

    struct NullFixture;

    impl NullFixture {
        const URL: &'static str = "https://github.example.com/example-org/example-project/pull/142";
    }

    #[test]
    fn every_url_fails_with_what_the_host_lacks() {
        let browser = NullWebBrowser::new(WebBrowserError::NoBrowser, "the example host lacks one");

        assert_eq!(
            browser.open(NullFixture::URL),
            Err(WebBrowserError::NoBrowser)
        );
    }

    #[test]
    fn every_failed_open_logs_why_the_host_lacks_a_browser() {
        let log = LogCapture::installed();
        let reason = "the example host has no browser for the null adapter";
        let browser = NullWebBrowser::new(WebBrowserError::NoBrowser, reason);

        browser.open(NullFixture::URL).unwrap_err();

        assert_eq!(
            log.count_containing(&format!("cannot open a web browser: {reason}")),
            1
        );
    }
}

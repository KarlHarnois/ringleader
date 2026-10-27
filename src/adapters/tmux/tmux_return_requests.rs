use super::tmux_command::TmuxCommand;
use super::tmux_error::TmuxError;
use super::tmux_output::TmuxOutput;
use super::tmux_sequence::TmuxSequence;
use crate::application::ports::{MultiplexerError, ReturnRequest};

pub struct TmuxReturnRequests<'a> {
    tmux: &'a TmuxCommand,
}

impl<'a> TmuxReturnRequests<'a> {
    const OPTION: &'static str = "@ringleader_return_request";
    const DELETE: &'static str = "delete ";

    pub fn new(tmux: &'a TmuxCommand) -> Self {
        Self { tmux }
    }

    pub fn record(&self, request: &ReturnRequest) -> Result<(), MultiplexerError> {
        let ReturnRequest::Delete(tmux_name) = request;
        let value = format!("{}{tmux_name}", Self::DELETE);
        let record = TmuxSequence::new().command(["set-option", "-g", Self::OPTION, &value]);
        let output = self.tmux.change(&record)?;
        Self::require_success("set-option", &output)
    }

    pub fn request(&self) -> Result<Option<ReturnRequest>, MultiplexerError> {
        let read = TmuxSequence::new().command(["show-options", "-gqv", Self::OPTION]);
        let output = self.tmux.query(&read)?;
        if output.reports_no_server() {
            return Ok(None);
        }
        Self::require_success("show-options", &output)?;
        Ok(Self::parsed(output.stdout()))
    }

    pub fn clear(&self) -> Result<(), MultiplexerError> {
        let clear = TmuxSequence::new().command(["set-option", "-gu", Self::OPTION]);
        let output = self.tmux.change(&clear)?;
        if output.reports_no_server() {
            return Ok(());
        }
        Self::require_success("set-option", &output)
    }

    fn parsed(value: &str) -> Option<ReturnRequest> {
        let line = value.strip_suffix('\n').unwrap_or(value);
        line.strip_prefix(Self::DELETE)
            .map(|tmux_name| ReturnRequest::Delete(tmux_name.to_string()))
    }

    fn require_success(subcommand: &str, output: &TmuxOutput) -> Result<(), MultiplexerError> {
        match output.succeeded() {
            true => Ok(()),
            false => Err(TmuxError::failed(subcommand, output).into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TmuxTestServer;

    struct RequestsFixture {
        _server: TmuxTestServer,
        tmux: TmuxCommand,
    }

    impl RequestsFixture {
        fn with_server() -> Self {
            Self::on(TmuxTestServer::with_session("holder"))
        }

        fn without_server() -> Self {
            Self::on(TmuxTestServer::new())
        }

        fn on(server: TmuxTestServer) -> Self {
            let tmux = server.command();
            Self {
                _server: server,
                tmux,
            }
        }

        fn requests(&self) -> TmuxReturnRequests<'_> {
            TmuxReturnRequests::new(&self.tmux)
        }

        fn delete(tmux_name: &str) -> ReturnRequest {
            ReturnRequest::Delete(tmux_name.to_string())
        }
    }

    #[test]
    fn a_recorded_request_reads_back_with_its_raw_tmux_name() {
        let fixture = RequestsFixture::with_server();
        let request = RequestsFixture::delete("ringleader-fix auth #{x}");

        fixture.requests().record(&request).unwrap();

        assert_eq!(fixture.requests().request().unwrap(), Some(request));
    }

    #[test]
    fn a_server_without_a_request_reads_as_none() {
        let fixture = RequestsFixture::with_server();

        assert_eq!(fixture.requests().request().unwrap(), None);
    }

    #[test]
    fn a_cleared_request_reads_as_none() {
        let fixture = RequestsFixture::with_server();
        let request = RequestsFixture::delete("ringleader-fix-auth");
        fixture.requests().record(&request).unwrap();

        fixture.requests().clear().unwrap();

        assert_eq!(fixture.requests().request().unwrap(), None);
    }

    #[test]
    fn a_missing_server_holds_no_request_to_read_or_clear() {
        let fixture = RequestsFixture::without_server();

        let cleared = fixture.requests().clear();

        assert_eq!(cleared, Ok(()));
        assert_eq!(fixture.requests().request().unwrap(), None);
    }
}

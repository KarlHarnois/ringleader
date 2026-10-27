use super::tmux_command::TmuxCommand;
use super::tmux_error::TmuxError;
use super::tmux_listing::TmuxListing;
use super::tmux_output::TmuxOutput;
use super::tmux_sequence::TmuxSequence;
use crate::application::ports::{ClientView, MultiplexerError};

pub struct TmuxClients<'a> {
    tmux: &'a TmuxCommand,
}

impl<'a> TmuxClients<'a> {
    const VIEW_LABEL: &'static str = "ringleader-client";
    const SHOWS_SHELL: &'static str = "1";
    const NAME_SEPARATOR: char = '\t';

    pub fn new(tmux: &'a TmuxCommand) -> Self {
        Self { tmux }
    }

    pub fn view(&self, client: &str) -> Result<Option<ClientView>, MultiplexerError> {
        let output = self.tmux.query(&Self::client_listing())?;
        if output.succeeded() {
            return Ok(Self::view_of(&output, client));
        }
        if output.reports_no_server() {
            return Ok(None);
        }
        Err(TmuxError::failed("list-clients", &output).into())
    }

    pub fn detach(&self, client: &str) -> Result<(), MultiplexerError> {
        let detach = TmuxSequence::new().command(["detach-client", "-t", client]);
        let output = self.tmux.change(&detach)?;
        match output.succeeded() {
            true => Ok(()),
            false => Err(TmuxError::failed("detach-client", &output).into()),
        }
    }

    fn client_listing() -> TmuxSequence {
        let format = format!(
            "{} #{{?{},1,0}} #{{client_name}}{}#{{session_name}}",
            Self::VIEW_LABEL,
            TmuxListing::SHELL_TAG,
            Self::NAME_SEPARATOR
        );
        TmuxSequence::new().command(["list-clients", "-F", &format])
    }

    fn view_of(output: &TmuxOutput, client: &str) -> Option<ClientView> {
        output.labeled(Self::VIEW_LABEL).find_map(|row| {
            let (shell_tag, names) = row.split_once(' ')?;
            let (name, tmux_name) = names.split_once(Self::NAME_SEPARATOR)?;
            (name == client).then(|| Self::shown(shell_tag, tmux_name))
        })
    }

    fn shown(shell_tag: &str, tmux_name: &str) -> ClientView {
        match shell_tag == Self::SHOWS_SHELL {
            true => ClientView::showing_shell(tmux_name),
            false => ClientView::showing_agent(tmux_name),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::test_support::TmuxTestServer;
    use crate::test_support::tmux_test_server::ControlClient;

    struct ClientsFixture {
        server: TmuxTestServer,
        tmux: TmuxCommand,
    }

    impl ClientsFixture {
        const SESSION: &'static str = "ringleader-fix auth";
        const DETACH_LIMIT: Duration = Duration::from_secs(2);

        fn new() -> Self {
            let server = TmuxTestServer::with_session(Self::SESSION);
            let tmux = server.command();
            Self { server, tmux }
        }

        fn without_server() -> Self {
            let server = TmuxTestServer::new();
            let tmux = server.command();
            Self { server, tmux }
        }

        fn clients(&self) -> TmuxClients<'_> {
            TmuxClients::new(&self.tmux)
        }

        fn client(&self) -> ControlClient {
            self.server.control_client(&format!("={}:", Self::SESSION))
        }

        fn view(&self, client: &ControlClient) -> Option<ClientView> {
            let name = self.server.client_name(client);
            self.clients().view(&name).unwrap()
        }

        fn open_shell_window(&self) {
            let target = format!("={}:", Self::SESSION);
            self.server.text(&["new-window", "-t", &target]);
            self.server.text(&[
                "set-option",
                "-w",
                "-t",
                &target,
                TmuxListing::SHELL_TAG,
                "1",
            ]);
        }
    }

    #[test]
    fn a_client_on_the_agent_reads_as_its_raw_tmux_session_showing_the_agent() {
        let fixture = ClientsFixture::new();
        let client = fixture.client();

        let view = fixture.view(&client);

        assert_eq!(
            view,
            Some(ClientView::showing_agent(ClientsFixture::SESSION))
        );
    }

    #[test]
    fn a_client_on_the_shell_window_reads_as_showing_the_shell() {
        let fixture = ClientsFixture::new();
        let client = fixture.client();

        fixture.open_shell_window();

        assert_eq!(
            fixture.view(&client),
            Some(ClientView::showing_shell(ClientsFixture::SESSION))
        );
    }

    #[test]
    fn an_unknown_client_reads_as_no_view() {
        let fixture = ClientsFixture::new();
        let _client = fixture.client();

        let view = fixture.clients().view("/dev/pts/999").unwrap();

        assert_eq!(view, None);
    }

    #[test]
    fn a_missing_server_reads_as_no_view() {
        let fixture = ClientsFixture::without_server();

        let view = fixture.clients().view("/dev/pts/999").unwrap();

        assert_eq!(view, None);
    }

    #[test]
    fn detaching_a_client_ends_its_attachment() {
        let fixture = ClientsFixture::new();
        let mut client = fixture.client();
        let name = fixture.server.client_name(&client);

        fixture.clients().detach(&name).unwrap();

        assert!(client.exits_with_notice_within(ClientsFixture::DETACH_LIMIT));
    }

    #[test]
    fn detaching_an_unknown_client_fails() {
        let fixture = ClientsFixture::new();

        let detached = fixture.clients().detach("/dev/pts/999");

        assert!(detached.is_err());
    }
}

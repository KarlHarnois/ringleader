use std::thread;
use std::time::{Duration, Instant};

use super::tmux_command::TmuxCommand;
use super::tmux_error::TmuxError;
use super::tmux_listing::TmuxListing;
use super::tmux_scripts::TmuxScripts;

pub struct ClientSizeFit<'a> {
    tmux: &'a TmuxCommand,
    pane: &'a str,
}

impl<'a> ClientSizeFit<'a> {
    const CLIENT_PATIENCE: Duration = Duration::from_secs(1);
    const CLIENT_POLL: Duration = Duration::from_millis(10);

    pub fn new(tmux: &'a TmuxCommand, pane: &'a str) -> Self {
        Self { tmux, pane }
    }

    pub fn attached_clients(&self) -> usize {
        self.tmux
            .query(&TmuxScripts::attached_clients(self.pane))
            .ok()
            .and_then(|output| TmuxListing::new(&output).attached_clients())
            .unwrap_or_default()
    }

    pub fn fit_once_a_client_joins(&self, clients_before: usize) {
        if self.client_joined(clients_before) {
            self.fit();
        }
    }

    pub fn fit(&self) {
        if let Err(error) = self.redraw_at_client_size() {
            log::warn!("{error}");
        }
    }

    fn client_joined(&self, clients_before: usize) -> bool {
        let deadline = Instant::now() + Self::CLIENT_PATIENCE;
        while self.attached_clients() <= clients_before {
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Self::CLIENT_POLL);
        }
        true
    }

    // Each resize runs in its own tmux call because tmux merges the resizes of one call.
    // An agent that the preview narrowed lays out its rows for the wide window only after
    // a second resize at the client's size.
    fn redraw_at_client_size(&self) -> Result<(), TmuxError> {
        for step in TmuxScripts::redraw_at_client_size(self.pane) {
            let output = self.tmux.change(&step)?;
            if !output.succeeded() {
                return Err(TmuxError::failed("resize-window", &output));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ClientSizeFit;
    use crate::test_support::TmuxTestServer;
    use crate::test_support::tmux_test_server::ControlClient;

    struct FitFixture {
        server: TmuxTestServer,
    }

    impl FitFixture {
        const WINDOW: &str = "=alpha:";
        const CLIENT_WIDTH: &str = "90";
        const CLIENT_HEIGHT: &str = "30";

        fn narrowed() -> Self {
            let server = TmuxTestServer::new();
            server.start_session("alpha");
            server.text(&["resize-window", "-t", Self::WINDOW, "-x", "46", "-y", "22"]);
            server.text(&["set-option", "-w", "-u", "-t", Self::WINDOW, "window-size"]);
            Self { server }
        }

        fn join_client(&self) -> ControlClient {
            let client = self.server.control_client(Self::WINDOW);
            let name = self.server.client_name(&client);
            let size = format!("{}x{}", Self::CLIENT_WIDTH, Self::CLIENT_HEIGHT);
            self.server
                .text(&["refresh-client", "-t", &name, "-C", &size]);
            client
        }

        fn window_size(&self) -> String {
            self.server.text(&[
                "display-message",
                "-p",
                "-t",
                Self::WINDOW,
                "#{window_width}x#{window_height}",
            ])
        }

        fn shows_a_second_resize_at_the_client_size(&self) -> bool {
            let shorter = format!("{}x29", Self::CLIENT_WIDTH);
            let client = format!("{}x{}", Self::CLIENT_WIDTH, Self::CLIENT_HEIGHT);
            self.server.wait_for(|| {
                self.server
                    .recorded_window_sizes()
                    .ends_with(&[shorter.clone(), client.clone()])
            })
        }
    }

    #[test]
    fn a_client_that_joins_gets_its_size_twice_so_a_narrowed_agent_lays_out_its_rows_again() {
        let fixture = FitFixture::narrowed();
        let tmux = fixture.server.command();
        let fit = ClientSizeFit::new(&tmux, FitFixture::WINDOW);
        let clients_before = fit.attached_clients();
        let _client = fixture.join_client();
        fixture.server.record_window_sizes();

        fit.fit_once_a_client_joins(clients_before);

        assert!(fixture.shows_a_second_resize_at_the_client_size());
    }

    #[test]
    fn a_window_that_no_client_joins_keeps_its_size() {
        let fixture = FitFixture::narrowed();
        let tmux = fixture.server.command();
        let fit = ClientSizeFit::new(&tmux, FitFixture::WINDOW);

        fit.fit_once_a_client_joins(fit.attached_clients());

        assert_eq!(fixture.window_size(), "46x22");
    }
}

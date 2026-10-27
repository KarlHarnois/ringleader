use std::thread;
use std::time::{Duration, Instant};

use super::tmux_command::TmuxCommand;
use super::tmux_error::TmuxError;
use super::tmux_listing::TmuxListing;
use super::tmux_scripts::TmuxScripts;

pub struct ClientSizeFit {
    tmux: TmuxCommand,
    pane: String,
    before: WindowWatch,
    patience: Duration,
}

impl ClientSizeFit {
    const CLIENT_PATIENCE: Duration = Duration::from_secs(1);
    const CLIENT_POLL: Duration = Duration::from_millis(10);

    pub fn before_a_client_arrives(tmux: &TmuxCommand, pane: &str) -> Self {
        Self {
            tmux: tmux.clone(),
            pane: pane.to_string(),
            before: WindowWatch::read(tmux, pane),
            patience: Self::CLIENT_PATIENCE,
        }
    }

    #[cfg(test)]
    pub fn with_patience(self, patience: Duration) -> Self {
        Self { patience, ..self }
    }

    pub fn fit_in_background_once_a_client_joins(self) {
        thread::spawn(move || self.fit_once_a_client_joins());
    }

    pub fn fit_once_a_client_joins(&self) {
        if self.client_joined() {
            self.fit_if_resized();
        }
    }

    pub fn fit_if_resized(&self) {
        if self.window_resized() {
            self.fit();
        }
    }

    fn client_joined(&self) -> bool {
        let deadline = Instant::now() + self.patience;
        while self.watch().clients() <= self.before.clients() {
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Self::CLIENT_POLL);
        }
        true
    }

    fn window_resized(&self) -> bool {
        self.watch().size() != self.before.size()
    }

    fn watch(&self) -> WindowWatch {
        WindowWatch::read(&self.tmux, &self.pane)
    }

    fn fit(&self) {
        if let Err(error) = self.redraw_at_client_size() {
            log::warn!("{error}");
        }
    }

    // Each resize runs in its own tmux call because tmux merges the resizes of one call.
    // An agent that the preview narrowed lays out its rows for the wide window only after
    // a second resize at the client's size.
    fn redraw_at_client_size(&self) -> Result<(), TmuxError> {
        for step in TmuxScripts::redraw_at_client_size(&self.pane) {
            let output = self.tmux.change(&step)?;
            if !output.succeeded() {
                return Err(TmuxError::failed("resize-window", &output));
            }
        }
        Ok(())
    }
}

struct WindowWatch {
    clients: usize,
    size: Option<String>,
}

impl WindowWatch {
    fn read(tmux: &TmuxCommand, pane: &str) -> Self {
        let output = tmux.query(&TmuxScripts::window_watch(pane)).ok();
        let listing = output.as_ref().map(TmuxListing::new);
        Self {
            clients: listing
                .as_ref()
                .and_then(TmuxListing::attached_clients)
                .unwrap_or_default(),
            size: listing.as_ref().and_then(TmuxListing::window_size),
        }
    }

    fn clients(&self) -> usize {
        self.clients
    }

    fn size(&self) -> Option<&str> {
        self.size.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

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
        const SENTINEL_SIZE: &str = "70x20";

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

        fn sizes_recorded_through_a_sentinel(&self) -> Vec<String> {
            let (width, height) = Self::SENTINEL_SIZE.split_once('x').unwrap();
            self.server.text(&[
                "resize-window",
                "-t",
                Self::WINDOW,
                "-x",
                width,
                "-y",
                height,
            ]);
            self.server.wait_for(|| {
                self.server
                    .recorded_window_sizes()
                    .contains(&Self::SENTINEL_SIZE.to_string())
            });
            self.server.recorded_window_sizes()
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
        let fit = ClientSizeFit::before_a_client_arrives(&tmux, FitFixture::WINDOW);
        let _client = fixture.join_client();
        fixture.server.record_window_sizes();

        fit.fit_once_a_client_joins();

        assert!(fixture.shows_a_second_resize_at_the_client_size());
    }

    #[test]
    fn a_window_that_no_client_joins_keeps_its_size() {
        let fixture = FitFixture::narrowed();
        let tmux = fixture.server.command();
        let fit = ClientSizeFit::before_a_client_arrives(&tmux, FitFixture::WINDOW)
            .with_patience(Duration::ZERO);

        fit.fit_once_a_client_joins();

        assert_eq!(fixture.window_size(), "46x22");
    }

    #[test]
    fn a_window_already_at_the_client_size_is_not_resized_again() {
        let fixture = FitFixture::narrowed();
        let _client = fixture.join_client();
        let tmux = fixture.server.command();
        let fit = ClientSizeFit::before_a_client_arrives(&tmux, FitFixture::WINDOW);
        fixture.server.record_window_sizes();

        fit.fit_if_resized();

        assert_eq!(
            fixture.sizes_recorded_through_a_sentinel(),
            [FitFixture::SENTINEL_SIZE]
        );
    }
}

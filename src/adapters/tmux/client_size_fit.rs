use std::thread;
use std::time::{Duration, Instant};

use super::tmux_command::TmuxCommand;
use super::tmux_error::TmuxError;
use super::tmux_listing::TmuxListing;
use super::tmux_scripts::TmuxScripts;
use crate::domain::SessionSize;

pub struct ClientSizeFit {
    tmux: TmuxCommand,
    pane: String,
    before: Option<WindowWatch>,
    patience: Duration,
}

impl ClientSizeFit {
    const CLIENT_PATIENCE: Duration = Duration::from_secs(1);
    const CLIENT_POLL: Duration = Duration::from_millis(10);

    pub fn before_a_client_arrives(tmux: &TmuxCommand, pane: &str) -> Self {
        Self {
            tmux: tmux.clone(),
            pane: pane.to_string(),
            before: WindowWatch::read(tmux, pane).inspect_err(Self::warn).ok(),
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
        let Some(before) = &self.before else {
            return;
        };
        if self.client_joined(before) {
            self.fit_resized_from(before);
        }
    }

    pub fn fit_if_resized(&self) {
        if let Some(before) = &self.before {
            self.fit_resized_from(before);
        }
    }

    fn client_joined(&self, before: &WindowWatch) -> bool {
        let deadline = Instant::now() + self.patience;
        while !self
            .watch()
            .is_ok_and(|now| now.clients() > before.clients())
        {
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Self::CLIENT_POLL);
        }
        true
    }

    fn fit_resized_from(&self, before: &WindowWatch) {
        let resized = self
            .watch()
            .inspect_err(Self::warn)
            .ok()
            .map(|now| now.size());
        if let Some(size) = resized.filter(|size| *size != before.size()) {
            self.fit(size);
        }
    }

    fn watch(&self) -> Result<WindowWatch, TmuxError> {
        WindowWatch::read(&self.tmux, &self.pane)
    }

    fn fit(&self, size: SessionSize) {
        if let Err(error) = self.redraw_at(size) {
            Self::warn(&error);
        }
    }

    // Each resize runs in its own tmux call because tmux merges the resizes of one call.
    // An agent that the preview narrowed lays out its rows for the wide window only after
    // a second resize at the client's size.
    fn redraw_at(&self, size: SessionSize) -> Result<(), TmuxError> {
        for step in TmuxScripts::redraw_at(&self.pane, size) {
            let output = self.tmux.change(&step)?;
            if !output.succeeded() {
                return Err(TmuxError::failed("resize-window", &output));
            }
        }
        Ok(())
    }

    fn warn(error: &TmuxError) {
        log::warn!("cannot fit the attached window to its client: {error}");
    }
}

struct WindowWatch {
    clients: usize,
    size: SessionSize,
}

impl WindowWatch {
    const SUBCOMMAND: &'static str = "display-message";

    fn read(tmux: &TmuxCommand, pane: &str) -> Result<Self, TmuxError> {
        let output = tmux.query(&TmuxScripts::window_watch(pane))?;
        let listing = TmuxListing::new(&output);
        match (listing.attached_clients(), listing.window_size()) {
            (Some(clients), Some(size)) => Ok(Self { clients, size }),
            _ => Err(TmuxError::failed(Self::SUBCOMMAND, &output)),
        }
    }

    fn clients(&self) -> usize {
        self.clients
    }

    fn size(&self) -> SessionSize {
        self.size
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
        const SESSION: &str = "alpha";
        const WINDOW: &str = "=alpha:";
        const SENTINEL_WINDOW: &str = "sentinel";
        const CLIENT_SIZE: &str = "90x30";
        const RESIZE_SIZE: &str = "70x20";

        fn narrowed() -> Self {
            let server = TmuxTestServer::new();
            server.start_session(Self::SESSION);
            server.text(&["resize-window", "-t", Self::WINDOW, "-x", "46", "-y", "22"]);
            server.text(&["set-option", "-w", "-u", "-t", Self::WINDOW, "window-size"]);
            Self { server }
        }

        fn join_client(&self) -> ControlClient {
            self.join_client_of_size(Self::CLIENT_SIZE)
        }

        fn join_client_of_size(&self, size: &str) -> ControlClient {
            let client = self.server.control_client(Self::WINDOW);
            let name = self.server.client_name(&client);
            self.server
                .text(&["refresh-client", "-t", &name, "-C", size]);
            client
        }

        fn window_size(&self) -> String {
            self.server.window_size(Self::WINDOW)
        }

        fn resize_window(&self, target: &str, size: &str) {
            let (width, height) = size.split_once('x').unwrap();
            self.server
                .text(&["resize-window", "-t", target, "-x", width, "-y", height]);
            let resized = self
                .server
                .wait_for(|| self.server.window_size(target) == size);
            assert!(resized, "window {target} did not reach {size}");
        }
    }

    struct SentinelArrangement {
        fixture: FitFixture,
        sentinel: String,
        _client: ControlClient,
    }

    impl SentinelArrangement {
        fn after_the_fit() -> Self {
            let fixture = FitFixture::narrowed();
            let sentinel = fixture
                .server
                .add_detached_window(FitFixture::SESSION, FitFixture::SENTINEL_WINDOW);
            let client = fixture.join_client();
            let tmux = fixture.server.command();
            let fit = ClientSizeFit::before_a_client_arrives(&tmux, FitFixture::WINDOW);
            fixture.server.record_window_resizes();
            fit.fit_if_resized();
            Self {
                fixture,
                sentinel,
                _client: client,
            }
        }
    }

    #[test]
    fn a_client_that_joins_gets_its_size_twice_so_a_narrowed_agent_lays_out_its_rows_again() {
        let fixture = FitFixture::narrowed();
        let tmux = fixture.server.command();
        let fit = ClientSizeFit::before_a_client_arrives(&tmux, FitFixture::WINDOW);
        let _client = fixture.join_client();
        fixture.server.record_window_resizes();

        fit.fit_once_a_client_joins();

        fixture
            .server
            .require_window_at(FitFixture::WINDOW, 2, FitFixture::CLIENT_SIZE);
    }

    #[test]
    fn a_client_that_joins_a_larger_client_gets_its_own_size_and_never_the_larger_one() {
        let fixture = FitFixture::narrowed();
        let _larger = fixture.join_client_of_size("120x40");
        let tmux = fixture.server.command();
        let fit = ClientSizeFit::before_a_client_arrives(&tmux, FitFixture::WINDOW);
        let _client = fixture.join_client();
        fixture.server.record_window_resizes();

        fit.fit_once_a_client_joins();

        fixture
            .server
            .require_window_at(FitFixture::WINDOW, 2, FitFixture::CLIENT_SIZE);
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
        let arrangement = SentinelArrangement::after_the_fit();
        let sentinel = arrangement.sentinel.as_str();

        arrangement
            .fixture
            .resize_window(sentinel, FitFixture::RESIZE_SIZE);
        arrangement
            .fixture
            .server
            .require_window_at(sentinel, 1, FitFixture::RESIZE_SIZE);
        let resizes = arrangement
            .fixture
            .server
            .window_resize_count(FitFixture::WINDOW);
        assert_eq!(resizes, 0, "count {resizes}");
    }

    #[test]
    fn a_resize_of_the_window_under_test_is_counted_once() {
        let arrangement = SentinelArrangement::after_the_fit();

        arrangement
            .fixture
            .resize_window(FitFixture::WINDOW, FitFixture::RESIZE_SIZE);
        arrangement
            .fixture
            .resize_window(&arrangement.sentinel, FitFixture::RESIZE_SIZE);
        arrangement.fixture.server.require_window_at(
            &arrangement.sentinel,
            1,
            FitFixture::RESIZE_SIZE,
        );
        let resizes = arrangement
            .fixture
            .server
            .window_resize_count(FitFixture::WINDOW);
        assert_eq!(resizes, 1, "count {resizes}");
    }
}

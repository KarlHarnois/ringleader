use std::time::Instant;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::Frame;

use super::action_gate::ActionGate;
use super::home_state::HomeState;
use super::home_view::{HomeView, LayoutTier};
use super::preview_panel::PreviewPanel;
use super::status_bar::{StatusBar, StatusMessage, StatusNote};
use super::theme::Theme;
use crate::application::{ReturnMode, ServiceError, SessionService};

pub struct App {
    service: SessionService,
    home: HomeState,
    message: Option<StatusMessage>,
    tick: u64,
    theme: Theme,
}

impl App {
    const RELOADED: &'static str = "Reloaded the config and the registry.";
    const RELOAD_FAILED: &'static str = "reload failed, kept the previous config";

    pub fn new(service: SessionService, now: Instant) -> Self {
        Self {
            service,
            home: HomeState::new(now),
            message: None,
            tick: 0,
            theme: Theme,
        }
    }

    pub fn with_message(self, message: Option<String>) -> Self {
        Self {
            message: message.map(StatusMessage::Error),
            ..self
        }
    }

    pub fn is_inside_tmux(&self) -> bool {
        self.service.is_inside_tmux()
    }

    pub fn handle_event(&mut self, event: &Event) -> AppRequest {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.handle_key(key),
            Event::Resize(width, height) => {
                self.resize(*width, *height);
                AppRequest::Redraw
            }
            _ => AppRequest::Continue,
        }
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.home
            .resize(LayoutTier::for_size(width, height), &self.service);
    }

    pub fn on_tick(&mut self, now: Instant) {
        self.tick = self.tick.wrapping_add(1);
        let outcome = self.home.tick(now, &mut self.service);
        self.report_failure(outcome);
    }

    pub fn attach(&mut self, name: &str) {
        self.message = match self.service.attach(name, ReturnMode::None) {
            Ok(outcome) => outcome
                .binding_warning()
                .map(|warning| StatusMessage::Warning(warning.to_string())),
            Err(error) => Some(StatusMessage::Error(error.to_string())),
        };
        self.observe();
    }

    pub fn perform(&mut self, operation: BlockingOperation) {
        match operation {
            BlockingOperation::Restart(name) => self.restart(&name),
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let sessions = self.home.sessions();
        let selected = sessions.selected();
        let prefix = self.service.config().naming().tmux_prefix();
        let status = StatusBar::new(self.theme)
            .with_message(self.message.as_ref())
            .with_note(selected.and_then(|row| StatusNote::new(row, prefix).text()));
        HomeView::new(
            sessions.table(self.tick, self.theme),
            PreviewPanel::new(selected, self.home.preview(), self.theme),
            status,
        )
        .render(frame);
    }

    fn handle_key(&mut self, key: &KeyEvent) -> AppRequest {
        self.message = None;
        match key.code {
            KeyCode::Char('q') => AppRequest::Quit,
            KeyCode::Down | KeyCode::Right | KeyCode::Char('j' | 'l') => self.move_selection(1),
            KeyCode::Up | KeyCode::Left | KeyCode::Char('k' | 'h') => self.move_selection(-1),
            KeyCode::Enter => self.request_attach(),
            KeyCode::Char('s') => self.request_restart(),
            KeyCode::Char('r') => self.reload(),
            _ => AppRequest::Continue,
        }
    }

    fn move_selection(&mut self, offset: isize) -> AppRequest {
        self.home.move_selection(offset, &self.service);
        AppRequest::Continue
    }

    fn request_attach(&mut self) -> AppRequest {
        match self.gate().attach() {
            Ok(name) => AppRequest::Attach(name),
            Err(refusal) => self.refuse(refusal),
        }
    }

    fn request_restart(&mut self) -> AppRequest {
        match self.gate().restart() {
            Ok(name) => self.block(
                format!("Restarting {name}…"),
                BlockingOperation::Restart(name),
            ),
            Err(refusal) => self.refuse(refusal),
        }
    }

    fn gate(&self) -> ActionGate<'_> {
        ActionGate::new(
            self.home.sessions().selected(),
            self.service.read_only_reason(),
        )
    }

    fn block(&mut self, busy: String, operation: BlockingOperation) -> AppRequest {
        self.message = Some(StatusMessage::Info(busy));
        AppRequest::Block(operation)
    }

    fn refuse(&mut self, reason: String) -> AppRequest {
        self.message = Some(StatusMessage::Error(reason));
        AppRequest::Continue
    }

    fn restart(&mut self, name: &str) {
        let outcome = self.service.restart_session(name);
        self.observe();
        self.message = Some(StatusMessage::reporting(
            outcome,
            format!("Restarted {name}."),
        ));
    }

    fn reload(&mut self) -> AppRequest {
        let outcome = self
            .service
            .reload()
            .map_err(|error| format!("{}: {error}", Self::RELOAD_FAILED));
        self.observe();
        self.message = Some(StatusMessage::reporting(
            outcome,
            Self::RELOADED.to_string(),
        ));
        AppRequest::Continue
    }

    fn observe(&mut self) {
        let outcome = self.home.observe(&mut self.service);
        self.report_failure(outcome);
    }

    fn report_failure(&mut self, outcome: Result<(), ServiceError>) {
        if let Err(error) = outcome {
            self.message = Some(StatusMessage::Error(error.to_string()));
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppRequest {
    Continue,
    Redraw,
    Quit,
    Attach(String),
    Block(BlockingOperation),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockingOperation {
    Restart(String),
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::application::ports::ConfigError;
    use crate::domain::{Config, ConfigDraft};
    use crate::test_support::{FakeMultiplexer, ServiceFixture, UiHarness};

    struct AppFixture;

    impl AppFixture {
        fn two_waiting_sessions() -> ServiceFixture {
            let first = ServiceFixture::session("dark-mode");
            let second = ServiceFixture::session("fix-auth");
            ServiceFixture::new()
                .with_sessions(&[first.clone(), second.clone()])
                .with_multiplexer(
                    FakeMultiplexer::new()
                        .with_live(ServiceFixture::waiting(&first))
                        .with_live(ServiceFixture::waiting(&second))
                        .with_capture("ringleader-dark-mode", "dark mode output")
                        .with_capture("ringleader-fix-auth", "fix auth output"),
                )
        }

        fn started(fixture: &ServiceFixture, width: u16, height: u16) -> App {
            let mut app = UiHarness::app(fixture);
            app.resize(width, height);
            app.on_tick(Instant::now());
            app
        }

        fn screen(app: &App, width: u16, height: u16) -> String {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| app.render(frame)).unwrap();
            let buffer = terminal.backend().buffer();
            buffer
                .content()
                .chunks(usize::from(width))
                .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        }

        fn status_line(app: &App, width: u16) -> String {
            let screen = Self::screen(app, width, 28);
            screen.lines().last().unwrap().trim_end().to_string()
        }

        fn captures(fixture: &ServiceFixture) -> Vec<String> {
            fixture
                .multiplexer()
                .calls()
                .into_iter()
                .filter(|call| call.starts_with("capture_pane"))
                .collect()
        }

        fn press(app: &mut App, character: char) {
            app.handle_event(&UiHarness::character(character));
        }

        fn selected(app: &App) -> Option<String> {
            app.home.sessions().selected_name().map(str::to_string)
        }

        fn config_error() -> ConfigError {
            ConfigError::Parse {
                path: PathBuf::from("/config/ringleader/config.toml"),
                detail: "expected `=`".to_string(),
            }
        }

        fn config_with_prefix(tmux_prefix: &str) -> Config {
            let draft = ConfigDraft::new(Path::new(ServiceFixture::WORKTREE_ROOT))
                .tmux_prefix(tmux_prefix)
                .agent(ServiceFixture::agent());
            Config::new(&draft).unwrap()
        }
    }

    #[test]
    fn the_full_tier_previews_the_selected_session() {
        let fixture = AppFixture::two_waiting_sessions();
        let app = AppFixture::started(&fixture, 100, 28);

        let screen = AppFixture::screen(&app, 100, 28);

        assert!(screen.contains("dark mode output"));
    }

    #[test]
    fn a_selection_change_captures_the_new_row_at_once() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, 'j');

        assert_eq!(
            AppFixture::captures(&fixture).last().unwrap(),
            "capture_pane ringleader-fix-auth"
        );
        assert!(AppFixture::screen(&app, 100, 28).contains("fix auth output"));
    }

    #[test]
    fn the_preview_refreshes_on_its_own_timer() {
        let fixture = AppFixture::two_waiting_sessions();
        let start = Instant::now();
        let mut app = AppFixture::started(&fixture, 100, 28);
        let before = AppFixture::captures(&fixture).len();

        app.on_tick(start + Duration::from_millis(800));

        assert_eq!(AppFixture::captures(&fixture).len(), before + 1);
    }

    #[test]
    fn a_hidden_preview_captures_nothing() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 60, 12);

        AppFixture::press(&mut app, 'j');

        assert!(AppFixture::captures(&fixture).is_empty());
    }

    #[test]
    fn growing_into_the_full_tier_captures_at_once() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 60, 12);

        app.handle_event(&Event::Resize(100, 28));

        assert_eq!(AppFixture::captures(&fixture).len(), 1);
    }

    #[test]
    fn vim_keys_and_arrows_move_the_selection() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, 'l');
        let after_l = AppFixture::selected(&app);
        AppFixture::press(&mut app, 'h');
        let after_h = AppFixture::selected(&app);
        app.handle_event(&UiHarness::key(KeyCode::Right));
        let after_right = AppFixture::selected(&app);
        app.handle_event(&UiHarness::key(KeyCode::Left));

        assert_eq!(after_l.as_deref(), Some("fix-auth"));
        assert_eq!(after_h.as_deref(), Some("dark-mode"));
        assert_eq!(after_right.as_deref(), Some("fix-auth"));
        assert_eq!(AppFixture::selected(&app).as_deref(), Some("dark-mode"));
    }

    #[test]
    fn the_selection_follows_its_session_when_rows_reorder() {
        let fixture = AppFixture::two_waiting_sessions();
        let start = Instant::now();
        let mut app = AppFixture::started(&fixture, 100, 28);
        AppFixture::press(&mut app, 'j');
        fixture.multiplexer().end_session("ringleader-dark-mode");

        app.on_tick(start + Duration::from_secs(2));

        assert_eq!(app.home.sessions().rows()[0].name(), "fix-auth");
        assert_eq!(AppFixture::selected(&app).as_deref(), Some("fix-auth"));
    }

    #[test]
    fn the_status_bar_explains_ctrl_q_for_a_session_under_an_earlier_prefix() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_config(AppFixture::config_with_prefix("agents-"))
            .with_sessions(std::slice::from_ref(&session))
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session)));

        let app = AppFixture::started(&fixture, 100, 28);

        assert_eq!(
            AppFixture::status_line(&app, 100),
            " Ctrl-Q does not return from ringleader-fix-auth, whose name lacks the current tmux_prefix agents-."
        );
    }

    #[test]
    fn the_status_bar_gives_the_kill_session_advice_for_a_shadowed_pair() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_multiplexer(
                FakeMultiplexer::new()
                    .with_live(ServiceFixture::waiting(&session).without_metadata()),
            );

        let app = AppFixture::started(&fixture, 100, 28);

        assert!(
            AppFixture::status_line(&app, 250).contains("Inspect it, then end it with tmux -S")
        );
    }

    #[test]
    fn s_on_an_exited_session_asks_for_a_restart_behind_a_busy_message() {
        let fixture = UiHarness::exited_session_fixture("fix-auth");
        let mut app = AppFixture::started(&fixture, 100, 28);

        let request = app.handle_event(&UiHarness::character('s'));

        assert_eq!(
            request,
            AppRequest::Block(BlockingOperation::Restart("fix-auth".to_string()))
        );
        assert_eq!(AppFixture::status_line(&app, 100), " Restarting fix-auth…");
    }

    #[test]
    fn performing_a_restart_respawns_the_agent_and_reports_it() {
        let fixture = UiHarness::exited_session_fixture("fix-auth");
        let mut app = AppFixture::started(&fixture, 100, 28);

        app.perform(BlockingOperation::Restart("fix-auth".to_string()));

        assert_eq!(fixture.multiplexer().launches().len(), 1);
        assert_eq!(AppFixture::status_line(&app, 100), " Restarted fix-auth.");
    }

    #[test]
    fn s_on_a_waiting_session_explains_the_refusal() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);

        let request = app.handle_event(&UiHarness::character('s'));

        assert_eq!(request, AppRequest::Continue);
        assert_eq!(
            AppFixture::status_line(&app, 100),
            " dark-mode is waiting, so it cannot be restarted."
        );
    }

    #[test]
    fn s_in_a_read_only_tui_shows_the_config_error_and_restarts_nothing() {
        let fixture = UiHarness::exited_session_fixture("fix-auth")
            .with_broken_config(AppFixture::config_error());
        let mut app = AppFixture::started(&fixture, 100, 28);

        let request = app.handle_event(&UiHarness::character('s'));

        assert_eq!(request, AppRequest::Continue);
        assert!(
            AppFixture::status_line(&app, 200).starts_with(" config error, running read-only:")
        );
    }

    #[test]
    fn r_reloads_the_config() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);
        fixture
            .config_source()
            .replace(AppFixture::config_with_prefix("agents-"));

        AppFixture::press(&mut app, 'r');

        assert_eq!(app.service.config().naming().tmux_prefix(), "agents-");
        assert_eq!(
            AppFixture::status_line(&app, 100),
            " Reloaded the config and the registry."
        );
    }

    #[test]
    fn r_on_a_broken_file_keeps_the_previous_config_and_shows_the_error() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);
        fixture
            .config_source()
            .break_with(AppFixture::config_error());

        AppFixture::press(&mut app, 'r');

        assert_eq!(app.service.config(), &ServiceFixture::config());
        assert!(
            AppFixture::status_line(&app, 200)
                .starts_with(" reload failed, kept the previous config:")
        );
    }

    #[test]
    fn r_on_a_fixed_config_lets_a_read_only_tui_restart_again() {
        let fixture = UiHarness::exited_session_fixture("fix-auth")
            .with_broken_config(AppFixture::config_error());
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, 'r');
        let request = app.handle_event(&UiHarness::character('s'));

        assert!(matches!(request, AppRequest::Block(_)));
    }

    #[test]
    fn a_key_press_clears_a_shown_message() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28)
            .with_message(Some("the config failed".to_string()));

        AppFixture::press(&mut app, 'j');

        assert!(AppFixture::status_line(&app, 100).starts_with(" n new"));
    }

    #[test]
    fn a_tui_on_a_broken_config_observes_without_persisting() {
        let adoptable = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_broken_config(ConfigError::Parse {
                path: PathBuf::from("/config/ringleader/config.toml"),
                detail: "expected `=`".to_string(),
            })
            .with_multiplexer(
                FakeMultiplexer::new().with_live(ServiceFixture::waiting(&adoptable)),
            );
        let mut app = UiHarness::app(&fixture);

        app.on_tick(Instant::now());

        assert_eq!(fixture.registry().save_count(), 0);
    }

    #[test]
    fn a_tui_on_a_valid_config_persists_an_adoption() {
        let adoptable = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new().with_live(ServiceFixture::waiting(&adoptable)),
        );
        let mut app = UiHarness::app(&fixture);

        app.on_tick(Instant::now());

        assert_eq!(fixture.registry().save_count(), 1);
    }
}

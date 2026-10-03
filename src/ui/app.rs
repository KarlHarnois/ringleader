use std::io;
use std::time::Instant;

use crossterm::event::{Event, KeyEvent, KeyEventKind};
use ratatui::Frame;

use super::action_gate::{ActionGate, KeyAction};
use super::blocking_operation::BlockingOperation;
use super::dialogs::{Dialog, DialogStep};
use super::home_state::HomeState;
use super::home_view::{HomeView, LayoutTier};
use super::status_bar::{StatusBar, StatusMessage};
use super::status_report::StatusReport;
use super::theme::Theme;
use crate::application::{EphemeralShell, ReturnMode, SessionService, ShellOutcome};

pub struct App {
    service: SessionService,
    home: HomeState,
    dialog: Option<Dialog>,
    status: StatusReport,
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
            dialog: None,
            status: StatusReport::default(),
            tick: 0,
            theme: Theme,
        }
    }

    pub fn with_message(mut self, message: Option<String>) -> Self {
        self.status.show(message.map(StatusMessage::Error));
        self
    }

    pub fn is_inside_tmux(&self) -> bool {
        self.service.is_inside_tmux()
    }

    pub fn handle_event(&mut self, event: &Event) -> AppRequest {
        match (event, &mut self.dialog) {
            (Event::Resize(width, height), _) => {
                self.resize(*width, *height);
                AppRequest::Redraw
            }
            (_, Some(dialog)) => match dialog.handle_event(event, &self.service) {
                DialogStep::Stay => AppRequest::Continue,
                DialogStep::Close => {
                    self.dialog = None;
                    AppRequest::Continue
                }
                DialogStep::Request(request) => self.request(request),
            },
            (Event::Key(key), None) if key.kind != KeyEventKind::Release => self.handle_key(key),
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
        self.status.failure(outcome);
        if let Some(dialog) = &mut self.dialog {
            dialog.poll(&mut self.service);
        }
    }

    pub fn attach(&mut self, name: &str) {
        let outcome = self.service.attach(name, ReturnMode::None);
        self.observe();
        match outcome {
            Ok(attached) => self.status.attached(&attached),
            Err(error) => self.status.error(error.to_string()),
        }
    }

    pub fn open_shell(&mut self, name: &str) -> Option<EphemeralShell> {
        let outcome = self.service.open_shell(name, ReturnMode::None);
        self.observe();
        match outcome {
            Ok(ShellOutcome::Ephemeral(shell)) => return Some(shell),
            Ok(ShellOutcome::Attached(attached)) => self.status.attached(&attached),
            Err(error) => self.status.error(error.to_string()),
        }
        None
    }

    pub fn shell_finished(&mut self, outcome: io::Result<()>) {
        if let Err(error) = outcome {
            self.status.error(format!("the shell failed: {error}"));
        }
        self.observe();
    }

    pub fn perform(&mut self, operation: BlockingOperation) {
        let outcome = operation.run(&mut self.service);
        self.observe();
        match (outcome, &mut self.dialog) {
            (Ok(done), _) => {
                self.dialog = None;
                self.home.select(done.session(), &self.service);
                self.status.info(done.message().to_string());
            }
            (Err(error), Some(dialog)) => {
                dialog.fail(error);
                self.status.show(None);
            }
            (Err(error), None) => self.status.error(error),
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let prefix = self.service.config().naming().tmux_prefix();
        let status = StatusBar::new(self.theme)
            .with_message(self.status.message())
            .with_note(self.home.note(prefix));
        let table = self.home.table(self.tick, self.theme);
        HomeView::new(table, self.home.preview_panel(self.theme), status).render(frame);
        if let Some(dialog) = &self.dialog {
            dialog.render(frame, &self.theme);
        }
    }

    fn handle_key(&mut self, key: &KeyEvent) -> AppRequest {
        self.status.show(None);
        let selected = self.home.sessions().selected();
        match ActionGate::new(selected, self.service.read_only_reason()).action_for(key) {
            KeyAction::Request(request) => return self.request(request),
            KeyAction::Move(offset) => self.home.move_selection(offset, &self.service),
            KeyAction::NewSession => self.dialog = Some(Dialog::new_session(&self.service)),
            KeyAction::Reload => self.reload(),
            KeyAction::Refuse(refusal) => self.status.error(refusal),
            KeyAction::Ignore => {}
        }
        AppRequest::Continue
    }

    fn request(&mut self, request: AppRequest) -> AppRequest {
        if let AppRequest::Block(operation) = &request {
            self.status.info(operation.busy_message());
        }
        request
    }

    fn reload(&mut self) {
        let outcome = self.service.reload();
        self.observe();
        let outcome = outcome.map_err(|error| format!("{}: {error}", Self::RELOAD_FAILED));
        self.status.outcome(outcome, Self::RELOADED.to_string());
    }

    fn observe(&mut self) {
        let outcome = self.home.observe(&mut self.service);
        self.status.failure(outcome);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppRequest {
    Continue,
    Redraw,
    Quit,
    Attach(String),
    Shell(String),
    Block(BlockingOperation),
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use crossterm::event::KeyCode;
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

        fn settle_dialog(app: &mut App) {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline && Self::screen(app, 100, 28).contains("Scanning") {
                app.on_tick(Instant::now());
                std::thread::sleep(Duration::from_millis(5));
            }
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
    fn t_asks_for_a_shell_in_the_selected_session() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);

        let request = app.handle_event(&UiHarness::character('t'));

        assert_eq!(request, AppRequest::Shell("dark-mode".to_string()));
    }

    #[test]
    fn n_then_enter_creates_the_session_selects_it_and_closes_the_dialog() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);
        AppFixture::press(&mut app, 'n');
        AppFixture::settle_dialog(&mut app);
        for character in "new-idea".chars() {
            AppFixture::press(&mut app, character);
        }

        let request = app.handle_event(&UiHarness::key(KeyCode::Enter));
        let busy = AppFixture::status_line(&app, 100);
        let AppRequest::Block(operation) = request else {
            panic!("expected a blocking create, got {request:?}");
        };
        app.perform(operation);

        assert_eq!(busy, " Creating new-idea…");
        assert!(app.dialog.is_none());
        assert_eq!(AppFixture::selected(&app).as_deref(), Some("new-idea"));
        assert_eq!(
            AppFixture::status_line(&app, 100),
            " Created new-idea on branch ringleader/new-idea."
        );
    }

    #[test]
    fn a_failed_create_keeps_the_dialog_open_with_the_error() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);
        AppFixture::press(&mut app, 'n');
        AppFixture::settle_dialog(&mut app);
        for character in "fix-auth".chars() {
            AppFixture::press(&mut app, character);
        }

        let AppRequest::Block(operation) = app.handle_event(&UiHarness::key(KeyCode::Enter)) else {
            panic!("expected a blocking create");
        };
        app.perform(operation);

        assert!(app.dialog.is_some());
        assert!(AppFixture::screen(&app, 100, 28).contains("fix-auth"));
        assert!(!AppFixture::status_line(&app, 100).contains("Created"));
    }

    #[test]
    fn n_in_a_read_only_tui_opens_no_dialog() {
        let fixture =
            AppFixture::two_waiting_sessions().with_broken_config(AppFixture::config_error());
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, 'n');

        assert!(app.dialog.is_none());
        assert!(
            AppFixture::status_line(&app, 200).starts_with(" config error, running read-only:")
        );
    }

    #[test]
    fn a_paste_goes_to_the_open_dialog() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);
        AppFixture::press(&mut app, 'n');

        app.handle_event(&Event::Paste("pasted\nname".to_string()));

        assert!(AppFixture::screen(&app, 100, 28).contains("pasted name"));
    }

    #[test]
    fn a_paste_on_the_home_screen_is_ignored() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);

        let request = app.handle_event(&Event::Paste("q".to_string()));

        assert_eq!(request, AppRequest::Continue);
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

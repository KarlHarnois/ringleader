use std::io::{self, Write};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use crossterm::event::{Event, KeyEvent, KeyEventKind};
use ratatui::Frame;

use super::action_gate::{ActionGate, KeyAction};
use super::bell::Bell;
use super::blocking_operation::{BlockingOperation, OperationOutcome};
use super::dialogs::{Dialog, DialogSlot, DialogStep};
use super::home_state::HomeState;
use super::home_view::HomeView;
use super::status_bar::{StatusBar, StatusMessage};
use super::status_report::StatusReport;
use super::theme::Theme;
use crate::application::ports::{BindingError, ReturnBinding};
use crate::application::{EphemeralShell, ReturnMode, ServiceError, SessionService, ShellOutcome};

pub struct App {
    service: SessionService,
    home: HomeState,
    dialogs: DialogSlot,
    status: StatusReport,
    binding: Option<BindingGuard>,
    tick: u64,
    theme: Theme,
}

impl App {
    const RELOADED: &'static str = "Reloaded the config and the registry.";
    const RELOAD_FAILED: &'static str = "reload failed, kept the previous config";

    pub fn new(service: SessionService, bell_sink: Box<dyn Write>, now: Instant) -> Self {
        Self {
            service,
            home: HomeState::new(now, Bell::new(bell_sink)),
            dialogs: DialogSlot::default(),
            status: StatusReport::default(),
            binding: None,
            tick: 0,
            theme: Theme,
        }
    }

    pub fn with_message(mut self, message: Option<String>) -> Self {
        self.status.show(message.map(StatusMessage::Error));
        self
    }

    pub fn with_binding_guard(mut self, binding: BindingGuard) -> Self {
        self.binding = Some(binding);
        self
    }

    pub fn is_inside_tmux(&self) -> bool {
        self.service.is_inside_tmux()
    }

    pub fn handle_event(&mut self, event: &Event) -> AppRequest {
        match event {
            Event::Resize(width, height) => {
                self.resize(*width, *height);
                AppRequest::Redraw
            }
            _ if self.dialogs.is_open() => match self.dialogs.handle_event(event, &self.service) {
                DialogStep::Request(request) => self.request(request),
                DialogStep::Stay | DialogStep::Close => AppRequest::Continue,
            },
            Event::Key(key) if key.kind != KeyEventKind::Release => self.handle_key(key),
            _ => AppRequest::Continue,
        }
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.home.resize(width, height, &self.service);
    }

    pub fn on_tick(&mut self, now: Instant) {
        self.tick = self.tick.wrapping_add(1);
        self.service.poll_startup_input();
        let outcome = self.home.tick(now, &mut self.service);
        self.status.failure(outcome);
        self.dialogs.poll();
    }

    pub fn attach(&mut self, name: &str) {
        let outcome = self.service.attach(name, ReturnMode::InstallReturnBinding);
        self.observe();
        match outcome {
            Ok(attached) => self.status.attached(&attached),
            Err(error) => self.report_failure(error),
        }
    }

    pub fn open_shell(&mut self, name: &str) -> Option<EphemeralShell> {
        let outcome = self
            .service
            .open_shell(name, ReturnMode::InstallReturnBinding);
        self.observe();
        match outcome {
            Ok(ShellOutcome::Ephemeral(shell)) => return Some(shell),
            Ok(ShellOutcome::Attached(attached)) => self.status.attached(&attached),
            Err(error) => self.report_failure(error),
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
        match outcome {
            OperationOutcome::Done { message, select } => {
                self.dialogs.close();
                if let Some(name) = select {
                    self.home.select(&name, &self.service);
                }
                self.status.info(message);
            }
            OperationOutcome::ConfirmDelete {
                name,
                impact,
                changed,
            } => {
                let row = self.home.sessions().row(&name);
                self.dialogs
                    .open(Dialog::delete(row, &name, impact, changed));
                self.status.show(None);
            }
            OperationOutcome::Failed(error) => self.report_failure(error),
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let prefix = self.service.config().naming().tmux_prefix();
        let status = StatusBar::new(self.theme)
            .with_message(self.status.message())
            .with_note(self.home.note(prefix));
        let table = self.home.table(self.tick, self.theme);
        HomeView::new(table, self.home.preview_panel(self.theme), status).render(frame);
        self.dialogs.render(frame, &self.theme);
    }

    fn handle_key(&mut self, key: &KeyEvent) -> AppRequest {
        self.status.show(None);
        let selected = self.home.sessions().selected();
        match ActionGate::new(selected, self.service.read_only_reason()).action_for(key) {
            KeyAction::Request(request) => return self.request(request),
            KeyAction::Move(offset) => self.home.move_selection(offset, &self.service),
            KeyAction::Open(opening) => {
                self.dialogs.open(Dialog::open(opening, &self.service));
            }
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
        self.home.refresh_now();
        self.observe();
        let outcome = outcome.map_err(|error| format!("{}: {error}", Self::RELOAD_FAILED));
        self.status.outcome(outcome, Self::RELOADED.to_string());
    }

    fn report_failure(&mut self, error: ServiceError) {
        self.home.pause_after(&error);
        match self.dialogs.fail(error.to_string()) {
            Ok(()) => self.status.show(None),
            Err(error) => self.status.error(error),
        }
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

pub struct BindingGuard {
    binding: Arc<dyn ReturnBinding>,
    notice: NoticeHandle,
}

impl BindingGuard {
    pub fn new(binding: Arc<dyn ReturnBinding>, notice: NoticeHandle) -> Self {
        Self { binding, notice }
    }

    pub fn repair_stale(&self) -> Option<String> {
        self.report(self.binding.repair_stale())
    }

    fn report(&self, outcome: Result<(), BindingError>) -> Option<String> {
        let error = outcome.err()?;
        log::warn!("{error}");
        if let BindingError::NotRestored { saved_line } = &error {
            self.notice.add(saved_line);
        }
        Some(error.to_string())
    }
}

impl Drop for BindingGuard {
    fn drop(&mut self) {
        self.report(self.binding.restore());
    }
}

pub struct ExitNotice {
    lines: Arc<Mutex<Vec<String>>>,
    sink: Box<dyn Write>,
}

impl ExitNotice {
    pub fn new(sink: Box<dyn Write>) -> Self {
        Self {
            lines: Arc::default(),
            sink,
        }
    }

    pub fn handle(&self) -> NoticeHandle {
        NoticeHandle {
            lines: Arc::clone(&self.lines),
        }
    }
}

impl Drop for ExitNotice {
    fn drop(&mut self) {
        let lines = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
        for line in lines.iter() {
            let _ = writeln!(
                self.sink,
                "ringleader: could not restore your Ctrl-Q binding, rebind it by hand: {line}"
            );
        }
    }
}

#[derive(Clone)]
pub struct NoticeHandle {
    lines: Arc<Mutex<Vec<String>>>,
}

impl NoticeHandle {
    fn add(&self, line: &str) {
        let mut lines = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
        lines.push(line.to_string());
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use crossterm::event::KeyCode;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::application::ports::{ConfigError, MultiplexerError, UnmergedCommits};
    use crate::domain::{Config, ConfigDraft, DiffStat};
    use crate::test_support::{
        CapturedWriter, FakeMultiplexer, FakeReturnBinding, ServiceFixture, UiHarness,
    };

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
            while Instant::now() < deadline
                && Self::screen(app, 100, 28).contains("Finding the repo")
            {
                app.on_tick(Instant::now());
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        fn deletable() -> ServiceFixture {
            let session = ServiceFixture::session("fix-auth");
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_workspace(ServiceFixture::workspace_for(&[&session]))
                .with_host(ServiceFixture::host_for(&[&session]))
                .with_multiplexer(
                    FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session)),
                )
        }

        fn perform(app: &mut App, request: AppRequest) {
            let AppRequest::Block(operation) = request else {
                panic!("expected a blocking operation, got {request:?}");
            };
            app.perform(operation);
        }

        fn tmux_timeout() -> MultiplexerError {
            MultiplexerError::TimedOut {
                subcommand: "list-panes".to_string(),
                seconds: 2,
            }
        }

        fn observe_calls(fixture: &ServiceFixture) -> usize {
            let calls = fixture.multiplexer().calls();
            calls
                .iter()
                .filter(|call| call.starts_with("observe_all"))
                .count()
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
    fn n_then_ctrl_s_creates_the_session_selects_it_and_closes_the_dialog() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);
        AppFixture::press(&mut app, 'n');
        AppFixture::settle_dialog(&mut app);
        for character in "new-idea".chars() {
            AppFixture::press(&mut app, character);
        }

        let request = app.handle_event(&UiHarness::control('s'));
        let busy = AppFixture::status_line(&app, 100);
        let AppRequest::Block(operation) = request else {
            panic!("expected a blocking create, got {request:?}");
        };
        app.perform(operation);

        assert_eq!(busy, " Creating new-idea…");
        assert!(!app.dialogs.is_open());
        assert_eq!(AppFixture::selected(&app).as_deref(), Some("new-idea"));
        assert_eq!(
            AppFixture::status_line(&app, 100),
            " Created new-idea on branch new-idea."
        );
    }

    #[test]
    fn the_busy_message_clears_before_the_startup_input_is_typed() {
        let first = ServiceFixture::session("dark-mode");
        let second = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(&[first.clone(), second.clone()])
            .with_config(ServiceFixture::config_with_input())
            .with_multiplexer(
                FakeMultiplexer::new()
                    .with_live(ServiceFixture::waiting(&first))
                    .with_live(ServiceFixture::waiting(&second))
                    .blocking_typing(),
            );
        let mut app = AppFixture::started(&fixture, 100, 28);
        AppFixture::press(&mut app, 'n');
        AppFixture::settle_dialog(&mut app);
        for character in "new-idea".chars() {
            AppFixture::press(&mut app, character);
        }

        let AppRequest::Block(operation) = app.handle_event(&UiHarness::control('s')) else {
            panic!("expected a blocking create");
        };
        app.perform(operation);

        assert_eq!(
            AppFixture::status_line(&app, 100),
            " Created new-idea on branch new-idea."
        );
        assert!(
            fixture
                .multiplexer()
                .typed_when_ready("ringleader-new-idea")
                .is_empty()
        );
        fixture.multiplexer().release_typing();
        fixture
            .multiplexer()
            .wait_for_typing("ringleader-new-idea", Duration::from_secs(2));
        app.on_tick(Instant::now());
        assert_eq!(
            fixture
                .multiplexer()
                .typed_when_ready("ringleader-new-idea"),
            ["/color"]
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

        let AppRequest::Block(operation) = app.handle_event(&UiHarness::control('s')) else {
            panic!("expected a blocking create");
        };
        app.perform(operation);

        assert!(app.dialogs.is_open());
        assert!(AppFixture::screen(&app, 100, 28).contains("fix-auth"));
        assert!(!AppFixture::status_line(&app, 100).contains("Created"));
    }

    #[test]
    fn n_in_a_read_only_tui_opens_no_dialog() {
        let fixture =
            AppFixture::two_waiting_sessions().with_broken_config(AppFixture::config_error());
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, 'n');

        assert!(!app.dialogs.is_open());
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
    fn d_measures_first_and_then_opens_the_confirmation() {
        let fixture = AppFixture::deletable();
        let mut app = AppFixture::started(&fixture, 100, 28);

        let request = app.handle_event(&UiHarness::character('d'));
        let busy = AppFixture::status_line(&app, 100);
        AppFixture::perform(&mut app, request);

        assert_eq!(busy, " Checking what deleting fix-auth would lose…");
        assert!(app.dialogs.is_open());
        assert!(AppFixture::screen(&app, 100, 28).contains("Delete session"));
    }

    #[test]
    fn confirming_deletes_the_session_and_reports_the_branch_tip() {
        let fixture = AppFixture::deletable();
        let mut app = AppFixture::started(&fixture, 100, 28);
        let measure = app.handle_event(&UiHarness::character('d'));
        AppFixture::perform(&mut app, measure);

        let delete = app.handle_event(&UiHarness::key(KeyCode::Enter));
        AppFixture::perform(&mut app, delete);

        assert!(!app.dialogs.is_open());
        assert!(fixture.registry().sessions().is_empty());
        assert!(AppFixture::status_line(&app, 200).starts_with(&format!(
            " deleted fix-auth, branch tip {}",
            ServiceFixture::TIP
        )));
    }

    #[test]
    fn changed_guards_reopen_the_dialog_with_the_new_impact() {
        let session = ServiceFixture::session("fix-auth");
        let workspace = ServiceFixture::workspace_for(&[&session]).with_unmerged(
            "fix-auth",
            vec![
                UnmergedCommits::new(0, Some(ServiceFixture::TIP), Some(ServiceFixture::TIP)),
                UnmergedCommits::new(2, Some(ServiceFixture::TIP), Some(ServiceFixture::TIP)),
            ],
        );
        let fixture = AppFixture::deletable().with_workspace(workspace);
        let mut app = AppFixture::started(&fixture, 100, 28);
        let measure = app.handle_event(&UiHarness::character('d'));
        AppFixture::perform(&mut app, measure);

        let delete = app.handle_event(&UiHarness::key(KeyCode::Enter));
        AppFixture::perform(&mut app, delete);

        let screen = AppFixture::screen(&app, 100, 28);
        assert!(screen.contains("2 commits exist on no other ref"));
        assert!(screen.contains("What delete would lose changed"));
        assert_eq!(fixture.registry().sessions().len(), 1);
    }

    #[test]
    fn a_refused_measure_shows_in_the_status_bar_instead_of_a_dialog() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = AppFixture::deletable().with_multiplexer(
            FakeMultiplexer::new()
                .with_live(ServiceFixture::waiting(&session))
                .with_caller("ringleader-fix-auth"),
        );
        let mut app = AppFixture::started(&fixture, 100, 28);

        let measure = app.handle_event(&UiHarness::character('d'));
        AppFixture::perform(&mut app, measure);

        assert!(!app.dialogs.is_open());
        assert_eq!(
            AppFixture::status_line(&app, 200),
            " session fix-auth runs this Ringleader, delete it from another terminal"
        );
    }

    #[test]
    fn an_unmeasurable_worktree_says_forget_needs_the_tui_to_quit() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = AppFixture::deletable().with_workspace(
            ServiceFixture::workspace_for(&[&session])
                .with_invalid_repo(Path::new(ServiceFixture::REPO)),
        );
        let mut app = AppFixture::started(&fixture, 100, 28);

        let measure = app.handle_event(&UiHarness::character('d'));
        AppFixture::perform(&mut app, measure);

        let status = AppFixture::status_line(&app, 400);
        assert!(status.contains("git cannot measure the worktree"));
        assert!(
            status.contains("ringleader delete fix-auth --forget needs this TUI to quit first")
        );
    }

    #[test]
    fn attach_from_the_tui_installs_the_return_binding_first() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);

        app.attach("dark-mode");

        assert_eq!(fixture.binding().calls(), ["install"]);
        assert_eq!(fixture.multiplexer().attached().len(), 1);
    }

    #[test]
    fn a_shell_from_the_tui_installs_the_return_binding_first() {
        let fixture = AppFixture::deletable();
        let mut app = AppFixture::started(&fixture, 100, 28);

        app.open_shell("fix-auth");

        assert_eq!(fixture.binding().calls(), ["install"]);
    }

    #[test]
    fn a_binding_that_cannot_be_installed_is_explained_in_the_status_bar() {
        let fixture = AppFixture::two_waiting_sessions().with_binding(
            FakeReturnBinding::default().failing_install(BindingError::Unavailable {
                reason: "TMUX_PANE is not set, as in a display-popup".to_string(),
            }),
        );
        let mut app = AppFixture::started(&fixture, 100, 28);

        app.attach("dark-mode");

        assert_eq!(
            AppFixture::status_line(&app, 200),
            " Ctrl-Q return binding not installed: TMUX_PANE is not set, as in a display-popup"
        );
    }

    #[test]
    fn dropping_the_app_restores_the_binding() {
        let fixture = AppFixture::two_waiting_sessions();
        let notice = ExitNotice::new(Box::new(CapturedWriter::default()));
        let app = UiHarness::app(&fixture)
            .with_binding_guard(BindingGuard::new(fixture.shared_binding(), notice.handle()));

        drop(app);

        assert_eq!(fixture.binding().calls(), ["restore"]);
    }

    #[test]
    fn a_binding_that_cannot_be_restored_reaches_the_exit_notice() {
        let fixture = AppFixture::two_waiting_sessions().with_binding(
            FakeReturnBinding::default().failing_restore(BindingError::NotRestored {
                saved_line: "bind-key -T root C-q send-keys example".to_string(),
            }),
        );
        let writer = CapturedWriter::default();
        let notice = ExitNotice::new(Box::new(writer.clone()));
        let guard = BindingGuard::new(fixture.shared_binding(), notice.handle());

        drop(guard);
        let before_the_notice_drops = writer.text();
        drop(notice);

        assert_eq!(before_the_notice_drops, "");
        assert_eq!(
            writer.text(),
            "ringleader: could not restore your Ctrl-Q binding, rebind it by hand: bind-key -T root C-q send-keys example\n"
        );
    }

    #[test]
    fn a_kept_record_at_startup_is_a_warning_and_never_a_notice() {
        let fixture = AppFixture::two_waiting_sessions().with_binding(
            FakeReturnBinding::default().failing_repair(BindingError::RecordKept {
                detail: "Permission denied".to_string(),
                saved_line: "bind-key -T root C-q send-keys example".to_string(),
            }),
        );
        let writer = CapturedWriter::default();
        let notice = ExitNotice::new(Box::new(writer.clone()));
        let guard = BindingGuard::new(fixture.shared_binding(), notice.handle());

        let warning = guard.repair_stale();
        drop(guard);
        drop(notice);

        assert_eq!(
            warning.as_deref(),
            Some("kept the saved Ctrl-Q binding for a later retry: Permission denied")
        );
        assert_eq!(writer.text(), "");
    }

    #[test]
    fn a_stale_binding_that_cannot_be_restored_at_startup_reaches_the_exit_notice() {
        let fixture = AppFixture::two_waiting_sessions().with_binding(
            FakeReturnBinding::default().failing_repair(BindingError::NotRestored {
                saved_line: "bind-key -T root C-q send-keys example".to_string(),
            }),
        );
        let writer = CapturedWriter::default();
        let notice = ExitNotice::new(Box::new(writer.clone()));
        let guard = BindingGuard::new(fixture.shared_binding(), notice.handle());

        assert!(guard.repair_stale().is_some());
        drop(guard);
        drop(notice);

        assert!(writer.text().contains("send-keys example"));
    }

    #[test]
    fn capital_r_renames_the_session_and_selects_its_new_name() {
        let fixture = AppFixture::deletable();
        let mut app = AppFixture::started(&fixture, 100, 28);
        AppFixture::press(&mut app, 'R');
        for character in "fix-login".chars() {
            AppFixture::press(&mut app, character);
        }

        let request = app.handle_event(&UiHarness::key(KeyCode::Enter));
        let busy = AppFixture::status_line(&app, 100);
        AppFixture::perform(&mut app, request);

        assert_eq!(busy, " Renaming fix-auth…");
        assert!(!app.dialogs.is_open());
        assert_eq!(AppFixture::selected(&app).as_deref(), Some("fix-login"));
        assert_eq!(
            AppFixture::status_line(&app, 100),
            " Renamed fix-auth to fix-login, branch fix-login."
        );
    }

    #[test]
    fn a_refused_rename_keeps_the_dialog_open_with_the_error() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);
        AppFixture::press(&mut app, 'R');
        for character in "fix-auth".chars() {
            AppFixture::press(&mut app, character);
        }

        let request = app.handle_event(&UiHarness::key(KeyCode::Enter));
        AppFixture::perform(&mut app, request);

        assert!(app.dialogs.is_open());
        assert_eq!(fixture.registry().sessions().len(), 2);
    }

    #[test]
    fn a_measured_diff_shows_in_the_table_and_survives_the_next_poll() {
        let session = ServiceFixture::session("fix-auth");
        let workspace = ServiceFixture::workspace_for(&[&session])
            .with_diff(session.worktree_path(), DiffStat::from_changes([], 3));
        let fixture = AppFixture::deletable().with_workspace(workspace);
        let start = Instant::now();
        let mut app = AppFixture::started(&fixture, 100, 28);
        let measured = AppFixture::screen(&app, 100, 28);

        app.on_tick(start + Duration::from_millis(1100));

        assert!(measured.contains("+0 -0 ?3"));
        assert!(AppFixture::screen(&app, 100, 28).contains("+0 -0 ?3"));
    }

    #[test]
    fn a_tmux_timeout_pauses_observing_until_r() {
        let fixture = AppFixture::two_waiting_sessions();
        let start = Instant::now();
        let mut app = AppFixture::started(&fixture, 100, 28);
        fixture
            .multiplexer()
            .fail("observe_all", AppFixture::tmux_timeout());

        app.on_tick(start + Duration::from_secs(2));
        let after_the_timeout = AppFixture::observe_calls(&fixture);
        app.on_tick(start + Duration::from_secs(4));
        let while_paused = AppFixture::observe_calls(&fixture);
        AppFixture::press(&mut app, 'r');

        assert_eq!(while_paused, after_the_timeout);
        assert_eq!(AppFixture::observe_calls(&fixture), after_the_timeout + 1);
    }

    #[test]
    fn a_preview_timeout_pauses_the_preview_too() {
        let fixture = AppFixture::two_waiting_sessions();
        fixture
            .multiplexer()
            .fail("capture_pane", AppFixture::tmux_timeout());
        let start = Instant::now();
        let mut app = AppFixture::started(&fixture, 100, 28);
        let first = AppFixture::captures(&fixture).len();

        app.on_tick(start + Duration::from_secs(1));
        app.on_tick(start + Duration::from_secs(2));

        assert_eq!(first, 1);
        assert_eq!(AppFixture::captures(&fixture).len(), 1);
    }

    #[test]
    fn question_mark_opens_the_help_and_closes_it_again() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, '?');
        let opened = AppFixture::screen(&app, 100, 28);
        AppFixture::press(&mut app, '?');

        assert!(opened.contains("┌ Help "));
        assert!(opened.contains("n           create a session"));
        assert!(!app.dialogs.is_open());
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

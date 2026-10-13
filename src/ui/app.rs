use std::io::{self, Write};
use std::time::Instant;

use crossterm::event::{Event, KeyEvent, KeyEventKind};
use ratatui::Frame;

use super::action_gate::{ActionGate, KeyAction};
use super::bell::Bell;
use super::blocking_operation::{BlockingOperation, OperationOutcome};
use super::dialogs::{Dialog, DialogSlot, DialogStep};
use super::home_state::HomeState;
use super::home_view::HomeView;
use super::key_sequence::KeySequence;
use super::status_bar::{StatusBar, StatusMessage};
use super::status_report::StatusReport;
use super::theme::Theme;
use crate::application::{EphemeralShell, ServiceError, SessionService, ShellOutcome};

pub struct App {
    service: SessionService,
    home: HomeState,
    key_sequence: KeySequence,
    dialogs: DialogSlot,
    status: StatusReport,
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
            key_sequence: KeySequence::default(),
            dialogs: DialogSlot::default(),
            status: StatusReport::default(),
            tick: 0,
            theme: Theme,
        }
    }

    pub fn with_message(mut self, message: Option<String>) -> Self {
        self.status.show(message.map(StatusMessage::Error));
        self
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

    pub fn into_service(self) -> SessionService {
        self.service
    }

    pub fn attach(&mut self, name: &str) {
        let outcome = self.service.attach(name);
        self.observe();
        if let Err(error) = outcome {
            self.report_failure(error);
        }
    }

    pub fn open_shell(&mut self, name: &str) -> Option<EphemeralShell> {
        let outcome = self.service.open_shell(name);
        self.observe();
        match outcome {
            Ok(ShellOutcome::Ephemeral(shell)) => return Some(shell),
            Ok(ShellOutcome::Attached) => {}
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
        let status = StatusBar::new(self.theme)
            .with_message(self.status.message())
            .with_note(self.home.note());
        let table = self.home.table(self.tick, self.theme);
        HomeView::new(table, self.home.preview_panel(self.theme), status).render(frame);
        self.dialogs.render(frame, &self.theme);
    }

    fn handle_key(&mut self, key: &KeyEvent) -> AppRequest {
        self.status.show(None);
        let gate = Self::gate(&self.home, &self.service);
        match self.key_sequence.handle_key(key, &gate) {
            KeyAction::Request(request) => return self.request(request),
            KeyAction::Move(offset) => self.home.move_selection(offset, &self.service),
            KeyAction::SelectFirst => self.home.select_first(&self.service),
            KeyAction::SelectLast => self.home.select_last(&self.service),
            KeyAction::Fold => self.home.toggle_fold(&self.service),
            KeyAction::JumpToRepo(number) => self.home.jump_to_repo(number, &self.service),
            KeyAction::Open(opening) => {
                self.dialogs.open(Dialog::open(opening, &self.service));
            }
            KeyAction::Reload => self.reload(),
            KeyAction::Refuse(refusal) => self.status.error(refusal),
            KeyAction::Ignore => {}
        }
        AppRequest::Continue
    }

    fn gate<'a>(home: &'a HomeState, service: &'a SessionService) -> ActionGate<'a> {
        let sessions = home.sessions();
        let config_error = service.read_only_reason();
        if sessions.heading_selected() {
            return ActionGate::on_heading(config_error);
        }
        ActionGate::new(sessions.selected(), config_error)
    }

    fn request(&mut self, request: AppRequest) -> AppRequest {
        if let AppRequest::Block(operation) = &request {
            self.status.info(operation.busy_message());
        }
        request
    }

    fn reload(&mut self) {
        let outcome = self.service.reload();
        let restored = self.service.restore_server_socket();
        self.home.refresh_now();
        self.observe();
        let reloaded = match &restored {
            Ok(Some(note)) => format!("{} Also {note}.", Self::RELOADED),
            _ => Self::RELOADED.to_string(),
        };
        let outcome = outcome
            .map_err(|error| format!("{}: {error}", Self::RELOAD_FAILED))
            .and_then(|()| restored.map(|_| ()).map_err(|error| error.to_string()))
            .and_then(|()| {
                self.service
                    .reapply_server_config()
                    .map_err(|error| error.to_string())
            });
        self.status.outcome(outcome, reloaded);
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use crossterm::event::KeyCode;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::application::ports::{ConfigError, MultiplexerError, UnmergedCommits};
    use crate::domain::{Config, ConfigDraft, DiffStat, PullRequestState, Session};
    use crate::test_support::{FakeMultiplexer, FakePullRequests, ServiceFixture, UiHarness};

    struct AppFixture;

    impl AppFixture {
        fn two_waiting_sessions() -> ServiceFixture {
            Self::waiting_sessions(&[
                ("dark-mode", "dark mode output"),
                ("fix-auth", "fix auth output"),
            ])
        }

        fn three_waiting_sessions() -> ServiceFixture {
            Self::waiting_sessions(&[
                ("dark-mode", "dark mode output"),
                ("fix-auth", "fix auth output"),
                ("spike-tui", "spike tui output"),
            ])
        }

        fn waiting_sessions(names_and_previews: &[(&str, &str)]) -> ServiceFixture {
            let sessions: Vec<Session> = names_and_previews
                .iter()
                .map(|(name, _)| ServiceFixture::session(name))
                .collect();
            let previews = names_and_previews.iter().map(|(_, preview)| *preview);
            let multiplexer = sessions
                .iter()
                .zip(previews)
                .fold(FakeMultiplexer::new(), |multiplexer, (session, preview)| {
                    Self::with_waiting(multiplexer, session, preview)
                });
            ServiceFixture::new()
                .with_sessions(&sessions)
                .with_multiplexer(multiplexer)
        }

        fn with_waiting(
            multiplexer: FakeMultiplexer,
            session: &Session,
            preview: &str,
        ) -> FakeMultiplexer {
            multiplexer
                .with_live(ServiceFixture::waiting(session))
                .with_capture(session.tmux_name(), preview)
        }

        fn two_repos() -> ServiceFixture {
            let docs =
                ServiceFixture::session_at("docs", "/repos/alpha", ServiceFixture::CREATED_AT);
            let fix_auth = ServiceFixture::session("fix-auth");
            ServiceFixture::new()
                .with_sessions(&[docs.clone(), fix_auth.clone()])
                .with_multiplexer(
                    FakeMultiplexer::new()
                        .with_live(ServiceFixture::waiting(&docs))
                        .with_live(ServiceFixture::waiting(&fix_auth)),
                )
        }

        fn blocked_startup_input() -> ServiceFixture {
            let first = ServiceFixture::session("dark-mode");
            let second = ServiceFixture::session("fix-auth");
            ServiceFixture::new()
                .with_sessions(&[first.clone(), second.clone()])
                .with_config(ServiceFixture::config_with_input())
                .with_multiplexer(
                    FakeMultiplexer::new()
                        .with_live(ServiceFixture::waiting(&first))
                        .with_live(ServiceFixture::waiting(&second))
                        .blocking_typing(),
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

        fn flattened_screen(app: &App, width: u16, height: u16) -> String {
            let screen = Self::screen(app, width, height);
            screen.split_whitespace().collect::<Vec<_>>().join(" ")
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

        fn request_create(app: &mut App, name: &str) -> AppRequest {
            Self::press(app, 'n');
            Self::settle_dialog(app);
            for character in name.chars() {
                Self::press(app, character);
            }
            app.handle_event(&UiHarness::control('s'))
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

        fn config_with_worktree_root(worktree_root: &str) -> Config {
            let draft = ConfigDraft::new(Path::new(worktree_root)).agent(ServiceFixture::agent());
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
    fn capital_g_selects_the_last_session() {
        let fixture = AppFixture::three_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, 'G');

        assert_eq!(AppFixture::selected(&app).as_deref(), Some("spike-tui"));
        assert_eq!(
            AppFixture::captures(&fixture).last().unwrap(),
            "capture_pane ringleader-spike-tui"
        );
    }

    #[test]
    fn gg_selects_the_first_session() {
        let fixture = AppFixture::three_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);
        AppFixture::press(&mut app, 'G');

        AppFixture::press(&mut app, 'g');
        AppFixture::press(&mut app, 'g');

        assert_eq!(AppFixture::selected(&app).as_deref(), Some("dark-mode"));
        assert_eq!(
            AppFixture::captures(&fixture).last().unwrap(),
            "capture_pane ringleader-dark-mode"
        );
    }

    #[test]
    fn space_folds_the_selected_repo() {
        let fixture = AppFixture::two_repos();
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, ' ');

        let screen = AppFixture::screen(&app, 100, 28);
        assert!(screen.contains("1 ▸ alpha (1)"));
        assert!(!screen.contains("docs"));
        assert!(screen.contains("fix-auth"));
    }

    #[test]
    fn a_digit_jumps_to_that_repo_and_previews_its_first_session() {
        let fixture = AppFixture::two_repos();
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, '2');

        assert_eq!(AppFixture::selected(&app).as_deref(), Some("fix-auth"));
        assert_eq!(
            AppFixture::captures(&fixture).last().unwrap(),
            "capture_pane ringleader-fix-auth"
        );
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
            AppFixture::flattened_screen(&app, 100, 28)
                .contains("Inspect it, then end it with tmux -S")
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
    fn a_restart_refused_for_a_lost_socket_keeps_its_advice_on_an_80_column_screen() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_host(ServiceFixture::with_worktree_agent(&session));
        let mut app = AppFixture::started(&fixture, 80, 24);

        app.perform(BlockingOperation::Restart("fix-auth".to_string()));

        assert!(AppFixture::flattened_screen(&app, 80, 24).contains("Do not end them"));
    }

    #[test]
    fn a_restart_refused_for_another_server_keeps_its_kill_command_on_an_80_column_screen() {
        let recorded = Path::new("/home/example/.local/state/ringleader/tmux/tmux.sock");
        let session = ServiceFixture::session("fix-auth").on_socket(recorded);
        let host = ServiceFixture::with_worktree_process(&session)
            .serving(ServiceFixture::WORKTREE_PROCESS, recorded);
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_host(host);
        let mut app = AppFixture::started(&fixture, 80, 24);

        app.perform(BlockingOperation::Restart("fix-auth".to_string()));

        assert!(
            AppFixture::flattened_screen(&app, 80, 24)
                .contains("kill-session -t '=ringleader-fix-auth:'")
        );
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
    fn r_recreates_a_lost_server_socket_and_says_so() {
        let fixture =
            ServiceFixture::new().with_multiplexer(FakeMultiplexer::new().with_lost_socket());
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, 'r');

        assert!(
            AppFixture::flattened_screen(&app, 100, 28)
                .contains("Reloaded the config and the registry. Also recreated the socket")
        );
    }

    #[test]
    fn r_reloads_the_config() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);
        fixture
            .config_source()
            .replace(AppFixture::config_with_worktree_root("/reloaded/worktrees"));

        AppFixture::press(&mut app, 'r');

        assert_eq!(
            app.service.config().naming().worktree_root(),
            Path::new("/reloaded/worktrees")
        );
        assert_eq!(
            AppFixture::status_line(&app, 100),
            " Reloaded the config and the registry."
        );
    }

    #[test]
    fn r_shows_a_failed_reapply_of_the_server_config() {
        let fixture = AppFixture::two_waiting_sessions();
        fixture.multiplexer().fail(
            "apply_config",
            MultiplexerError::Failed {
                subcommand: "source-file".to_string(),
                detail: "invalid option: no-such-option".to_string(),
            },
        );
        fixture.multiplexer().drift_config("M-x");
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, 'r');

        assert_eq!(
            AppFixture::status_line(&app, 200),
            " tmux source-file failed: invalid option: no-such-option"
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

        let request = AppFixture::request_create(&mut app, "new-idea");
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
        let fixture = AppFixture::blocked_startup_input();
        let mut app = AppFixture::started(&fixture, 100, 28);
        let request = AppFixture::request_create(&mut app, "new-idea");

        AppFixture::perform(&mut app, request);

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
        assert!(
            fixture
                .multiplexer()
                .wait_for_typing("ringleader-new-idea", Duration::from_secs(2))
        );
        assert_eq!(
            fixture
                .multiplexer()
                .typed_when_ready("ringleader-new-idea"),
            ["/color"]
        );
    }

    #[test]
    fn a_stopped_app_hands_back_its_queued_startup_input() {
        let fixture = AppFixture::blocked_startup_input();
        let mut app = AppFixture::started(&fixture, 100, 28);
        let request = AppFixture::request_create(&mut app, "new-idea");
        AppFixture::perform(&mut app, request);
        app.on_tick(Instant::now());

        app.into_service().wait_startup_input(|| {
            fixture.multiplexer().release_typing();
            false
        });

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
        let request = AppFixture::request_create(&mut app, "fix-auth");

        AppFixture::perform(&mut app, request);

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
    fn attach_from_the_tui_attaches_to_the_agent() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);

        app.attach("dark-mode");

        assert_eq!(fixture.multiplexer().attached().len(), 1);
        assert_eq!(
            AppFixture::status_line(&app, 100),
            " n new   d delete   R rename   Enter attach   t shell   s restart   r refresh   ? help   q quit"
        );
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
            " Renamed fix-auth to fix-login."
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
    fn a_session_with_a_pull_request_shows_its_number() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session)))
            .with_pull_requests(FakePullRequests::new().with_pull_request(
                Path::new(ServiceFixture::REPO),
                session.branch(),
                ServiceFixture::pull_request(142, PullRequestState::Open),
            ));
        let start = Instant::now();
        let mut app = AppFixture::started(&fixture, 100, 28);

        for step in 1..=200 {
            app.on_tick(start + Duration::from_millis(step * 5));
            if AppFixture::screen(&app, 100, 28).contains("#142") {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        assert_eq!(fixture.pull_requests().calls().len(), 1);
        assert!(AppFixture::screen(&app, 100, 28).contains("#142"));
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
        assert!(opened.contains("n            create a session"));
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

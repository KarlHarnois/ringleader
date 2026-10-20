use std::io::{self, Write};
use std::time::Instant;

use crossterm::event::{Event, KeyEvent, KeyEventKind};
use ratatui::Frame;

use super::blocking_operation::BlockingOperation;
use super::dialogs::{Dialog, DialogSlot, DialogStep};
use super::operation_outcome::OperationOutcome;
use super::theme::Theme;
use crate::application::ports::FoldStore;
use crate::application::{EphemeralShell, ServiceError, SessionService, ShellOutcome};
use crate::ui::home::bell::Bell;
use crate::ui::home::home_state::HomeState;
use crate::ui::home::home_view::HomeView;
use crate::ui::home::status_bar::{StatusBar, StatusMessage};
use crate::ui::home::status_report::StatusReport;
use crate::ui::keys::action_gate::{ActionGate, KeyAction, Navigation};
use crate::ui::keys::key_sequence::KeySequence;

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

    pub fn new(
        service: SessionService,
        bell_sink: Box<dyn Write>,
        folds: Box<dyn FoldStore>,
        now: Instant,
    ) -> Self {
        Self {
            service,
            home: HomeState::new(now, Bell::new(bell_sink), folds),
            key_sequence: KeySequence::default(),
            dialogs: DialogSlot::default(),
            status: StatusReport::default(),
            tick: 0,
            theme: Theme,
        }
    }

    pub fn with_startup_messages(mut self, warnings: &[String], note: Option<String>) -> Self {
        if warnings.is_empty() {
            self.status.show(note.map(StatusMessage::Info));
            return self;
        }
        let reported: Vec<String> = warnings.iter().cloned().chain(note).collect();
        self.status.error(reported.join("; "));
        self
    }

    pub fn handle_event(&mut self, event: &Event) -> AppRequest {
        match event {
            Event::Resize(width, height) => {
                self.resize(*width, *height);
                AppRequest::Redraw
            }
            _ if self.dialogs.is_open() => match self.dialogs.handle_event(event, &self.service) {
                DialogStep::Request(request) | DialogStep::CloseWith(request) => {
                    self.request(request)
                }
                DialogStep::Stay | DialogStep::Close => AppRequest::Continue,
            },
            Event::Key(key) if key.kind != KeyEventKind::Release => self.handle_key(key),
            _ => AppRequest::Continue,
        }
    }

    pub fn restore_folds(&mut self) {
        self.home.restore_folds();
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        let status = Self::status_bar(self.theme, &self.status, &self.home);
        self.home.resize(width, height, &status, &self.service);
    }

    pub fn on_tick(&mut self, now: Instant) {
        self.tick = self.tick.wrapping_add(1);
        self.service.poll_startup_input();
        let status = Self::status_bar(self.theme, &self.status, &self.home);
        let outcome = self.home.tick(now, &mut self.service, &status);
        self.status.failure(outcome);
        self.dialogs.poll(now, &self.service);
    }

    pub fn observe(&mut self) {
        let outcome = self.home.observe(&mut self.service);
        self.status.failure(outcome);
    }

    pub fn refresh_usage(&self) {
        self.service.refresh_usage();
    }

    pub fn poll(&mut self, now: Instant) {
        let outcome = self.home.poll(now, &mut self.service);
        self.status.failure(outcome);
    }

    pub fn awaits_results(&self) -> bool {
        self.home.awaits_results()
    }

    pub fn into_service(self) -> SessionService {
        self.service
    }

    pub fn attach(&mut self, name: &str) {
        let outcome = self.service.attach(name);
        self.home.returned_from(name);
        self.observe();
        self.home.select(name, &self.service);
        if let Err(error) = outcome {
            self.report_failure(error);
        }
    }

    pub fn open_shell(&mut self, name: &str) -> Option<EphemeralShell> {
        let outcome = self.service.open_shell(name);
        self.home.returned_from(name);
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
                self.settle(select);
                self.status.info(message);
            }
            OperationOutcome::Warned { message, select } => {
                self.settle(select);
                self.status.error(message);
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
        let status = Self::status_bar(self.theme, &self.status, &self.home);
        let table = self.home.table(self.tick, self.theme);
        HomeView::new(table, self.home.preview_panel(self.theme), status).render(frame);
        self.dialogs.render(frame, &self.theme);
    }

    fn status_bar<'s>(theme: Theme, status: &'s StatusReport, home: &HomeState) -> StatusBar<'s> {
        StatusBar::new(theme)
            .with_message(status.message())
            .with_note(home.note())
    }

    fn handle_key(&mut self, key: &KeyEvent) -> AppRequest {
        self.status.show(None);
        let gate = Self::gate(&self.home, &self.service);
        match self.key_sequence.handle_key(key, &gate) {
            KeyAction::Request(request) => self.request(request),
            KeyAction::Refuse(refusal) => {
                self.status.error(refusal);
                AppRequest::Continue
            }
            action => {
                self.act_on(action);
                AppRequest::Continue
            }
        }
    }

    fn act_on(&mut self, action: KeyAction) {
        match action {
            KeyAction::Navigate(navigation) => self.navigate(navigation),
            KeyAction::Fold => self.home.toggle_fold(&self.service),
            KeyAction::Open(opening) => {
                self.dialogs.open(Dialog::open(opening, &self.service));
            }
            KeyAction::FindSession => {
                let names = self.home.sessions().names();
                self.dialogs.open(Dialog::find_session(names));
            }
            KeyAction::Reload => self.reload(),
            KeyAction::OpenTerminal(name) => self.open_terminal(&name),
            KeyAction::Request(_) | KeyAction::Refuse(_) | KeyAction::Ignore => {}
        }
    }

    fn navigate(&mut self, navigation: Navigation) {
        match navigation {
            Navigation::Move(offset) => self.home.move_selection(offset, &self.service),
            Navigation::SelectFirst => self.home.select_first(&self.service),
            Navigation::SelectLast => self.home.select_last(&self.service),
            Navigation::SelectHigh => self.home.select_high(&self.service),
            Navigation::SelectMiddle => self.home.select_middle(&self.service),
            Navigation::SelectLow => self.home.select_low(&self.service),
            Navigation::JumpToRepo(number) => self.home.jump_to_repo(number, &self.service),
        }
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
                    .reload_server_config()
                    .map_err(|error| error.to_string())
            });
        self.status.outcome(outcome, reloaded);
    }

    fn open_terminal(&mut self, name: &str) {
        let outcome = self.service.open_terminal(name);
        let opened = format!("Opened a terminal window in the worktree of {name}.");
        self.status.outcome(outcome, opened);
    }

    fn settle(&mut self, select: Option<String>) {
        self.dialogs.close();
        self.home.refresh_diffs_soon();
        if let Some(name) = select {
            self.home.select(&name, &self.service);
        }
    }

    fn report_failure(&mut self, error: ServiceError) {
        self.home.pause_after(&error);
        match self.dialogs.fail(error.to_string()) {
            Ok(()) => self.status.show(None),
            Err(error) => self.status.error(error),
        }
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
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use crossterm::event::KeyCode;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use ratatui::style::Color;

    use crate::application::DeleteImpact;
    use crate::application::ports::{
        ConfigError, MergeOutcome, MultiplexerError, TerminalEmulatorError, UnmergedCommits,
        WebBrowserError,
    };
    use crate::domain::{
        AgentUsage, Config, ConfigDraft, DiffStat, LimitWindow, PullRequestState, Session,
        UsageLimit, UsageRow,
    };
    use crate::test_support::{
        CapturedWriter, Deadline, FakeFoldStore, FakeMultiplexer, FakePullRequests,
        FakeTerminalEmulator, FakeUsageMeter, FakeWebBrowser, HeldCalls, LogCapture,
        ServiceFixture, UiHarness,
    };
    use crate::ui::delete_step::DeleteStep;

    struct AppFixture;

    impl AppFixture {
        const HELP_PAST_THE_END: usize = 50;
        const SPINNER_FRAMES: &'static str = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";
        const LOG_FILE_OUTSIDE_THE_STATE_DIRECTORY: &'static str = "/logs/ringleader/app.log";

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

        fn log_file_outside_the_state_directory() -> ServiceFixture {
            let log_file = Path::new(Self::LOG_FILE_OUTSIDE_THE_STATE_DIRECTORY);
            ServiceFixture::new()
                .with_diagnostics(ServiceFixture::diagnostics().with_log_file(log_file))
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

        fn two_working_sessions_that_ring() -> ServiceFixture {
            let first = ServiceFixture::session("dark-mode");
            let second = ServiceFixture::session("fix-auth");
            let draft = ConfigDraft::new(Path::new(ServiceFixture::WORKTREE_ROOT))
                .bell_on_waiting(true)
                .agent(ServiceFixture::agent());
            ServiceFixture::new()
                .with_config(Config::new(&draft).unwrap())
                .with_sessions(&[first.clone(), second.clone()])
                .with_workspace(ServiceFixture::workspace_for(&[&first, &second]))
                .with_host(ServiceFixture::host_for(&[&first, &second]))
                .with_multiplexer(
                    FakeMultiplexer::new()
                        .with_live(ServiceFixture::working(&first))
                        .with_live(ServiceFixture::working(&second)),
                )
        }

        fn ringing(fixture: &ServiceFixture, bell: &CapturedWriter) -> App {
            let folds = Box::new(FakeFoldStore::default());
            let mut app = App::new(
                fixture.service(),
                Box::new(bell.clone()),
                folds,
                Instant::now(),
            );
            app.resize(100, 28);
            UiHarness::settle(&mut app);
            app
        }

        fn finish_working(fixture: &ServiceFixture, name: &str) {
            let session = ServiceFixture::session(name);
            fixture
                .multiplexer()
                .replace_live(ServiceFixture::waiting(&session));
        }

        fn started(fixture: &ServiceFixture, width: u16, height: u16) -> App {
            let mut app = UiHarness::app(fixture);
            app.resize(width, height);
            UiHarness::settle(&mut app);
            app
        }

        fn restored(fixture: &ServiceFixture, folds: &FakeFoldStore) -> App {
            let mut app = UiHarness::app_with_folds(fixture, folds);
            app.restore_folds();
            app.resize(UiHarness::WIDTH, UiHarness::HEIGHT);
            UiHarness::settle(&mut app);
            app
        }

        fn observed(fixture: &ServiceFixture) -> App {
            let mut app = UiHarness::app(fixture);
            app.resize(UiHarness::WIDTH, UiHarness::HEIGHT);
            app.observe();
            app
        }

        fn screens_until_settled(app: &mut App) -> Vec<String> {
            let mut screens = vec![Self::screen(app, UiHarness::WIDTH, UiHarness::HEIGHT)];
            Deadline::for_background_work().wait_until(|| {
                app.on_tick(Instant::now());
                screens.push(Self::screen(app, UiHarness::WIDTH, UiHarness::HEIGHT));
                !app.awaits_results()
            });
            screens
        }

        fn wait_for_capture(fixture: &ServiceFixture, name: &str) -> bool {
            let call = format!("capture_pane ringleader-{name}");
            Deadline::for_background_work().wait_until(|| Self::captures(fixture).contains(&call))
        }

        fn holding_captures(held: &Arc<HeldCalls>) -> ServiceFixture {
            let fixture = Self::two_waiting_sessions();
            fixture
                .multiplexer()
                .add_call_hook("capture_pane", held.hook());
            fixture
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

        fn status_color(app: &App, width: u16) -> Option<Color> {
            let mut terminal = Terminal::new(TestBackend::new(width, 28)).unwrap();
            terminal.draw(|frame| app.render(frame)).unwrap();
            let first_letter = terminal.backend().buffer().cell((1, 27)).unwrap();
            first_letter.style().fg
        }

        fn merge_missing_the_agent() -> ServiceFixture {
            let conflicts = MergeOutcome::Conflicted(vec!["src/auth.rs".to_string()]);
            ServiceFixture::merging(
                "fix-auth",
                conflicts,
                ServiceFixture::refusing_typing("fix-auth"),
            )
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

        fn measuring_usage() -> ServiceFixture {
            let limit = UsageLimit::used(LimitWindow::Session, 42.0, None);
            let usage = AgentUsage::new(vec![UsageRow::Limit(limit)]);
            Self::two_waiting_sessions()
                .with_usage_meter(FakeUsageMeter::new().with_usage("claude", usage))
        }

        fn spinner(screen: &str) -> Option<char> {
            screen
                .chars()
                .find(|glyph| Self::SPINNER_FRAMES.contains(*glyph))
        }

        fn press(app: &mut App, character: char) {
            app.handle_event(&UiHarness::character(character));
        }

        fn perform_key(app: &mut App, character: char) {
            let request = app.handle_event(&UiHarness::character(character));
            Self::perform(app, request);
        }

        fn looks_up_pull_requests_again(fixture: &ServiceFixture, app: &mut App) -> bool {
            let before = fixture.pull_requests().calls().len();
            Deadline::for_background_work().wait_until(|| {
                app.on_tick(Instant::now());
                fixture.pull_requests().calls().len() > before
            })
        }

        fn help_scrolled_past_its_end(app: &mut App, width: u16, height: u16) -> String {
            Self::press(app, '?');
            Self::screen(app, width, height);
            for _ in 0..Self::HELP_PAST_THE_END {
                Self::press(app, 'j');
            }
            Self::screen(app, width, height)
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
            Deadline::for_background_work().wait_until(|| {
                app.on_tick(Instant::now());
                !Self::screen(app, 100, 28).contains("Finding the repo")
            });
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

        fn with_pull_request() -> ServiceFixture {
            let session = ServiceFixture::session("fix-auth");
            Self::deletable().with_pull_requests(FakePullRequests::new().with_pull_request(
                Path::new(ServiceFixture::REPO),
                session.branch(),
                ServiceFixture::pull_request(142, PullRequestState::Open),
            ))
        }

        fn showing(fixture: &ServiceFixture, text: &str) -> App {
            let mut app = Self::started(fixture, 100, 28);
            Deadline::for_background_work().wait_until(|| {
                app.on_tick(Instant::now());
                Self::screen(&app, 100, 28).contains(text)
            });
            app
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
        let captured_without_a_tick = AppFixture::wait_for_capture(&fixture, "fix-auth");
        UiHarness::settle(&mut app);

        assert!(captured_without_a_tick);
        assert!(AppFixture::screen(&app, 100, 28).contains("fix auth output"));
    }

    #[test]
    fn j_moves_the_selection_while_a_capture_is_still_blocked() {
        let held = HeldCalls::all();
        let fixture = AppFixture::holding_captures(&held);
        let mut app = AppFixture::observed(&fixture);

        AppFixture::press(&mut app, 'j');
        let selected = AppFixture::selected(&app);
        let finished_captures = AppFixture::captures(&fixture).len();
        held.release();

        assert_eq!(selected.as_deref(), Some("fix-auth"));
        assert_eq!(finished_captures, 0);
    }

    #[test]
    fn a_capture_of_the_previous_selection_never_shows_under_the_new_one() {
        let held = HeldCalls::all();
        let fixture = AppFixture::holding_captures(&held);
        let mut app = AppFixture::observed(&fixture);
        AppFixture::press(&mut app, 'j');

        held.release();
        let screens = AppFixture::screens_until_settled(&mut app);

        assert!(
            screens
                .iter()
                .all(|screen| !screen.contains("dark mode output"))
        );
        assert!(screens.last().unwrap().contains("fix auth output"));
    }

    #[test]
    fn a_row_passed_over_during_a_capture_is_captured_once_that_capture_lands() {
        let fixture = AppFixture::three_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);
        let held = HeldCalls::all();
        fixture
            .multiplexer()
            .add_call_hook("capture_pane", held.hook());
        AppFixture::press(&mut app, 'j');
        AppFixture::press(&mut app, 'j');

        held.release();
        UiHarness::settle(&mut app);

        assert!(AppFixture::screen(&app, 100, 28).contains("spike tui output"));
    }

    #[test]
    fn a_late_capture_of_a_row_left_behind_keeps_the_current_preview() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);
        AppFixture::press(&mut app, 'j');
        UiHarness::settle(&mut app);
        let held = HeldCalls::all();
        fixture
            .multiplexer()
            .add_call_hook("capture_pane", held.hook());
        AppFixture::press(&mut app, 'k');
        AppFixture::press(&mut app, 'j');

        held.release();
        let screens = AppFixture::screens_until_settled(&mut app);

        assert!(
            screens
                .iter()
                .all(|screen| screen.contains("fix auth output"))
        );
    }

    #[test]
    fn a_shown_capture_leaves_the_preview_when_the_selection_moves() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);
        let held = HeldCalls::all();
        fixture
            .multiplexer()
            .add_call_hook("capture_pane", held.hook());

        AppFixture::press(&mut app, 'j');
        let screen = AppFixture::screen(&app, 100, 28);
        held.release();

        assert!(!screen.contains("dark mode output"));
    }

    #[test]
    fn the_preview_refreshes_on_its_own_timer() {
        let fixture = AppFixture::two_waiting_sessions();
        let start = Instant::now();
        let mut app = AppFixture::started(&fixture, 100, 28);
        let before = AppFixture::captures(&fixture).len();

        app.on_tick(start + Duration::from_millis(800));
        UiHarness::settle(&mut app);

        assert_eq!(AppFixture::captures(&fixture).len(), before + 1);
    }

    #[test]
    fn a_hidden_preview_captures_nothing() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 60, 12);

        AppFixture::press(&mut app, 'j');
        UiHarness::settle(&mut app);

        assert!(AppFixture::captures(&fixture).is_empty());
    }

    #[test]
    fn growing_into_the_full_tier_captures_at_once() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 60, 12);

        app.handle_event(&Event::Resize(100, 28));
        UiHarness::settle(&mut app);

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
        UiHarness::settle(&mut app);

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
        UiHarness::settle(&mut app);

        AppFixture::press(&mut app, 'g');
        AppFixture::press(&mut app, 'g');
        UiHarness::settle(&mut app);

        assert_eq!(AppFixture::selected(&app).as_deref(), Some("dark-mode"));
        assert_eq!(
            AppFixture::captures(&fixture).last().unwrap(),
            "capture_pane ringleader-dark-mode"
        );
    }

    #[test]
    fn capital_m_selects_the_middle_session_on_screen() {
        let fixture = AppFixture::three_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, 'M');
        UiHarness::settle(&mut app);

        assert_eq!(AppFixture::selected(&app).as_deref(), Some("fix-auth"));
        assert_eq!(
            AppFixture::captures(&fixture).last().unwrap(),
            "capture_pane ringleader-fix-auth"
        );
    }

    #[test]
    fn capital_h_selects_the_top_row_on_screen() {
        let fixture = AppFixture::three_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);
        AppFixture::press(&mut app, 'G');

        AppFixture::press(&mut app, 'H');

        assert_eq!(AppFixture::selected(&app).as_deref(), Some("dark-mode"));
    }

    #[test]
    fn capital_l_selects_the_bottom_session_on_screen() {
        let fixture = AppFixture::three_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, 'L');

        assert_eq!(AppFixture::selected(&app).as_deref(), Some("spike-tui"));
    }

    #[test]
    fn space_folds_the_selected_repo() {
        let fixture = UiHarness::two_repos();
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, ' ');

        let screen = AppFixture::screen(&app, 100, 28);
        assert!(screen.contains("1 ▸ alpha ●1"));
        assert!(!screen.contains("docs"));
        assert!(screen.contains("fix-auth"));
    }

    #[test]
    fn space_saves_the_folded_repo() {
        let fixture = UiHarness::two_repos();
        let folds = FakeFoldStore::default();
        let mut app = AppFixture::restored(&fixture, &folds);

        AppFixture::press(&mut app, ' ');

        assert_eq!(
            folds.folded(),
            HashSet::from([UiHarness::docs_repo_group()])
        );
    }

    #[test]
    fn folds_saved_by_an_earlier_run_show_at_start() {
        let fixture = UiHarness::two_repos();

        let app = AppFixture::restored(
            &fixture,
            &FakeFoldStore::holding(&[UiHarness::docs_repo_group()]),
        );

        let screen = AppFixture::screen(&app, 100, 28);
        assert!(screen.contains("1 ▸ alpha ●1"));
        assert!(!screen.contains("docs"));
    }

    #[test]
    fn observing_and_moving_without_a_fold_change_saves_nothing_so_ticks_write_no_file() {
        let fixture = UiHarness::two_repos();
        let folds = FakeFoldStore::holding(&[UiHarness::docs_repo_group()]);
        let mut app = AppFixture::restored(&fixture, &folds);

        AppFixture::press(&mut app, 'j');
        app.observe();

        assert_eq!(folds.save_count(), 0);
    }

    #[test]
    fn returning_from_a_session_in_a_folded_repo_saves_its_repo_unfolded() {
        let fixture = UiHarness::two_repos();
        let folds = FakeFoldStore::holding(&[UiHarness::docs_repo_group()]);
        let mut app = AppFixture::restored(&fixture, &folds);

        app.attach("docs");

        assert!(folds.folded().is_empty());
    }

    #[test]
    fn a_saved_fold_is_saved_as_forgotten_when_its_repo_starts_without_a_heading() {
        let fixture = AppFixture::two_waiting_sessions();
        let folds = FakeFoldStore::holding(&[ServiceFixture::repo_group(ServiceFixture::REPO)]);

        AppFixture::restored(&fixture, &folds);

        assert!(folds.folded().is_empty());
    }

    #[test]
    fn a_failed_fold_save_is_logged_and_the_fold_still_shows() {
        const DETAIL: &str = "the disk refused the fold save";
        let log = LogCapture::installed();
        let fixture = UiHarness::two_repos();
        let mut app = AppFixture::restored(&fixture, &FakeFoldStore::failing_saves(DETAIL));

        AppFixture::press(&mut app, ' ');

        let screen = AppFixture::screen(&app, 100, 28);
        assert!(screen.contains("1 ▸ alpha ●1"));
        assert!(!screen.contains(DETAIL));
        assert_eq!(log.count_containing(DETAIL), 1);
    }

    #[test]
    fn an_unreadable_fold_file_is_logged_and_every_repo_starts_unfolded() {
        const DETAIL: &str = "the disk refused the fold load";
        let log = LogCapture::installed();
        let fixture = UiHarness::two_repos();

        let app = AppFixture::restored(&fixture, &FakeFoldStore::failing_loads(DETAIL));

        assert!(AppFixture::screen(&app, 100, 28).contains("docs"));
        assert_eq!(log.count_containing(DETAIL), 1);
    }

    #[test]
    fn a_read_only_tui_still_saves_its_folds_because_a_fold_changes_no_session() {
        let fixture = UiHarness::two_repos().with_broken_config(AppFixture::config_error());
        let folds = FakeFoldStore::default();
        let mut app = AppFixture::restored(&fixture, &folds);

        AppFixture::press(&mut app, ' ');

        assert_eq!(folds.save_count(), 1);
    }

    #[test]
    fn a_digit_jumps_to_that_repo_and_previews_its_first_session() {
        let fixture = UiHarness::two_repos();
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, '2');
        UiHarness::settle(&mut app);

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
        UiHarness::settle(&mut app);

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
    fn p_on_a_waiting_session_asks_for_a_pull_behind_a_busy_message() {
        let fixture = AppFixture::deletable();
        let mut app = AppFixture::started(&fixture, 100, 28);

        let request = app.handle_event(&UiHarness::character('p'));

        assert_eq!(
            request,
            AppRequest::Block(BlockingOperation::Merge("fix-auth".to_string()))
        );
        assert_eq!(
            AppFixture::status_line(&app, 100),
            " Fetching origin and merging it into fix-auth…"
        );
    }

    #[test]
    fn a_clean_merge_reports_in_the_plain_status_color() {
        let fixture = AppFixture::deletable();
        let mut app = AppFixture::started(&fixture, 100, 28);

        app.perform(BlockingOperation::Merge("fix-auth".to_string()));

        assert_eq!(
            AppFixture::status_line(&app, 100),
            " merged origin/main into fix-auth"
        );
        assert_ne!(AppFixture::status_color(&app, 100), Theme.error().fg);
    }

    #[test]
    fn conflicts_the_agent_was_not_told_about_show_as_an_error() {
        let fixture = AppFixture::merge_missing_the_agent();
        let mut app = AppFixture::started(&fixture, 100, 28);

        app.perform(BlockingOperation::Merge("fix-auth".to_string()));

        assert_eq!(AppFixture::status_color(&app, 100), Theme.error().fg);
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
    fn shift_s_asks_to_restart_every_session_behind_a_busy_message() {
        let fixture = UiHarness::exited_session_fixture("fix-auth");
        let mut app = AppFixture::started(&fixture, 100, 28);

        let request = app.handle_event(&UiHarness::character('S'));

        assert_eq!(request, AppRequest::Block(BlockingOperation::RestartAll));
        assert_eq!(
            AppFixture::status_line(&app, 100),
            " Restarting every exited or stopped session…"
        );
    }

    #[test]
    fn performing_a_restart_of_every_session_reports_the_count() {
        let fixture = UiHarness::exited_session_fixture("fix-auth");
        let mut app = AppFixture::started(&fixture, 100, 28);

        app.perform(BlockingOperation::RestartAll);

        assert_eq!(fixture.multiplexer().launches().len(), 1);
        assert_eq!(AppFixture::status_line(&app, 100), " Restarted 1 session.");
    }

    #[test]
    fn a_restart_of_every_session_names_the_session_that_failed() {
        let fixture = ServiceFixture::new().with_sessions(&[ServiceFixture::session("fix-auth")]);
        let mut app = AppFixture::started(&fixture, 100, 28);

        app.perform(BlockingOperation::RestartAll);

        assert!(
            AppFixture::status_line(&app, 200)
                .starts_with(" Restarted 0 of 1 session. fix-auth failed: worktree "),
            "{}",
            AppFixture::status_line(&app, 200)
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
    fn a_restart_that_recreates_the_server_socket_says_so() {
        let fixture = UiHarness::exited_session_fixture("fix-auth");
        fixture.multiplexer().lose_socket();
        let mut app = AppFixture::started(&fixture, 100, 28);

        app.perform(BlockingOperation::Restart("fix-auth".to_string()));

        assert!(
            AppFixture::flattened_screen(&app, 100, 28)
                .contains("Restarted fix-auth. Also recreated the socket")
        );
    }

    #[test]
    fn a_restart_whose_envrc_is_not_allowed_shows_the_warning_in_the_status_bar() {
        let fixture = UiHarness::exited_session_fixture("fix-auth")
            .with_directory_environment(ServiceFixture::not_allowed_envrc("fix-auth"));
        let mut app = AppFixture::started(&fixture, 300, 28);

        app.perform(BlockingOperation::Restart("fix-auth".to_string()));

        assert!(
            AppFixture::flattened_screen(&app, 300, 28)
                .contains("Restarted fix-auth. But the .envrc of")
        );
    }

    #[test]
    fn returning_from_an_attach_does_not_ring_for_the_session_just_left() {
        let fixture = AppFixture::two_working_sessions_that_ring();
        let bell = CapturedWriter::default();
        let mut app = AppFixture::ringing(&fixture, &bell);
        AppFixture::finish_working(&fixture, "fix-auth");

        app.attach("fix-auth");

        assert_eq!(bell.text(), "");
    }

    #[test]
    fn returning_from_an_attach_still_rings_for_another_session_that_finished() {
        let fixture = AppFixture::two_working_sessions_that_ring();
        let bell = CapturedWriter::default();
        let mut app = AppFixture::ringing(&fixture, &bell);
        AppFixture::finish_working(&fixture, "fix-auth");
        AppFixture::finish_working(&fixture, "dark-mode");

        app.attach("fix-auth");

        assert_eq!(bell.text(), "\u{7}");
    }

    #[test]
    fn returning_from_a_shell_window_does_not_ring_for_its_session() {
        let fixture = AppFixture::two_working_sessions_that_ring();
        let bell = CapturedWriter::default();
        let mut app = AppFixture::ringing(&fixture, &bell);
        AppFixture::finish_working(&fixture, "fix-auth");

        let shell = app.open_shell("fix-auth");

        assert!(shell.is_none());
        assert_eq!(bell.text(), "");
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
    fn r_resources_the_server_config_even_when_its_hash_matches() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, 'r');

        assert!(
            fixture
                .multiplexer()
                .calls()
                .contains(&"apply_config C-q M-j M-Down M-k M-Up M-l".to_string())
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
    fn capital_t_opens_a_terminal_in_the_selected_worktree_and_stays_on_the_list() {
        let fixture = AppFixture::deletable();
        let mut app = AppFixture::started(&fixture, 100, 28);

        let request = app.handle_event(&UiHarness::character('T'));

        assert_eq!(request, AppRequest::Continue);
        assert_eq!(
            fixture.terminal_emulator().opened(),
            [ServiceFixture::worktree_path("fix-auth")]
        );
        assert_eq!(
            app.status.message(),
            Some(&StatusMessage::Info(
                "Opened a terminal window in the worktree of fix-auth.".to_string()
            ))
        );
    }

    #[test]
    fn capital_t_sends_nothing_to_tmux() {
        let fixture = AppFixture::deletable();
        let mut app = AppFixture::started(&fixture, 100, 28);
        let before = fixture.multiplexer().calls();

        AppFixture::press(&mut app, 'T');

        assert_eq!(fixture.multiplexer().calls(), before);
    }

    #[test]
    fn capital_t_on_an_exited_session_in_a_read_only_tui_opens_a_terminal() {
        let fixture = UiHarness::exited_session_fixture("fix-auth")
            .with_broken_config(AppFixture::config_error());
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, 'T');

        assert_eq!(fixture.terminal_emulator().opened().len(), 1);
    }

    #[test]
    fn a_terminal_that_cannot_open_shows_why_as_an_error() {
        let missing = TerminalEmulatorError::NoDisplay {
            reason: "the session runs on a text console".to_string(),
        };
        let fixture = AppFixture::deletable()
            .with_terminal_emulator(FakeTerminalEmulator::failing(missing.clone()));
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, 'T');

        assert_eq!(
            app.status.message(),
            Some(&StatusMessage::Error(missing.to_string()))
        );
    }

    #[test]
    fn capital_t_on_a_missing_worktree_gives_the_doctor_advice() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, 'T');

        assert_eq!(
            app.status.message(),
            Some(&StatusMessage::Error(format!(
                "worktree {} is missing, run ringleader doctor",
                ServiceFixture::worktree_path("dark-mode").display()
            )))
        );
        assert!(fixture.terminal_emulator().opened().is_empty());
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
    fn a_paste_goes_to_the_session_finder() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);
        AppFixture::press(&mut app, '/');
        app.handle_event(&Event::Paste("fix".to_string()));

        let request = app.handle_event(&UiHarness::key(KeyCode::Enter));

        assert_eq!(request, AppRequest::Attach("fix-auth".to_string()));
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
    fn a_delete_that_leaves_a_stray_session_reports_it_as_an_error() {
        let recorded = Path::new(ServiceFixture::OTHER_RUNTIME_SOCKET);
        let session = ServiceFixture::session("fix-auth").on_socket(recorded);
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_workspace(ServiceFixture::workspace_for(&[&session]))
            .with_host(ServiceFixture::host_for(&[&session]).with_tmux_server_on(recorded));
        let mut app = AppFixture::started(&fixture, 100, 28);

        app.perform(BlockingOperation::Delete(DeleteStep::Confirmed {
            name: "fix-auth".to_string(),
            seen: DeleteImpact::default(),
        }));

        assert!(matches!(
            app.status.message(),
            Some(StatusMessage::Error(text)) if text.contains("kill-session")
        ));
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
    fn an_observation_started_before_a_delete_does_not_bring_the_session_back() {
        let fixture = AppFixture::deletable();
        let start = Instant::now();
        let mut app = AppFixture::started(&fixture, 100, 28);
        let held = HeldCalls::first(1);
        fixture
            .multiplexer()
            .add_call_hook("observe_all", held.hook());
        app.on_tick(start + Duration::from_secs(2));
        let listing_started = held.wait_for(1);

        let measure = app.handle_event(&UiHarness::character('d'));
        AppFixture::perform(&mut app, measure);
        let delete = app.handle_event(&UiHarness::key(KeyCode::Enter));
        AppFixture::perform(&mut app, delete);
        held.release();
        UiHarness::settle(&mut app);

        assert!(listing_started);
        assert!(fixture.registry().sessions().is_empty());
        assert!(app.home.sessions().rows().is_empty());
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
        UiHarness::settle(&mut app);

        assert!(measured.contains("+0 -0 ?3"));
        assert!(AppFixture::screen(&app, 100, 28).contains("+0 -0 ?3"));
    }

    #[test]
    fn rows_hidden_under_a_wrapped_status_bar_get_no_pull_request_lookup() {
        let sessions =
            ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"].map(ServiceFixture::session);
        let multiplexer = sessions
            .iter()
            .fold(FakeMultiplexer::new(), |multiplexer, session| {
                multiplexer.with_live(ServiceFixture::waiting(session))
            });
        let fixture = ServiceFixture::new()
            .with_sessions(&sessions)
            .with_multiplexer(multiplexer);
        let mut app = UiHarness::app(&fixture).with_startup_messages(&["word ".repeat(40)], None);
        app.resize(100, 8);

        UiHarness::settle(&mut app);

        let looked_up = Deadline::after(Duration::from_secs(2))
            .wait_until(|| !fixture.pull_requests().calls().is_empty());
        assert!(looked_up);
        assert_eq!(
            fixture.pull_requests().calls(),
            [format!("lookup {} 4", ServiceFixture::REPO)]
        );
    }

    #[test]
    fn a_session_with_a_pull_request_shows_its_number() {
        let fixture = AppFixture::with_pull_request();

        let app = AppFixture::showing(&fixture, "#142");

        assert_eq!(fixture.pull_requests().calls().len(), 1);
        assert!(AppFixture::screen(&app, 100, 28).contains("#142"));
    }

    #[test]
    fn o_asks_to_open_the_pull_request_as_a_blocking_operation() {
        let fixture = AppFixture::with_pull_request();
        let mut app = AppFixture::showing(&fixture, "#142");

        let request = app.handle_event(&UiHarness::character('o'));

        assert_eq!(
            request,
            AppRequest::Block(BlockingOperation::OpenPullRequest("fix-auth".to_string()))
        );
    }

    #[test]
    fn o_opens_the_pull_request_of_the_selected_session_in_the_browser() {
        let fixture = AppFixture::with_pull_request();
        let mut app = AppFixture::showing(&fixture, "#142");

        AppFixture::perform_key(&mut app, 'o');

        assert_eq!(
            fixture.web_browser().opened(),
            [ServiceFixture::pull_request_url(142)]
        );
        assert_eq!(
            app.status.message(),
            Some(&StatusMessage::Info(
                "Asked the browser to open the pull request of fix-auth.".to_string()
            ))
        );
    }

    #[test]
    fn o_finds_a_pull_request_the_list_does_not_show_yet() {
        let fixture = AppFixture::with_pull_request();
        let mut app = AppFixture::observed(&fixture);

        AppFixture::perform_key(&mut app, 'o');

        assert_eq!(
            fixture.web_browser().opened(),
            [ServiceFixture::pull_request_url(142)]
        );
    }

    #[test]
    fn g_then_x_opens_the_pull_request_like_o() {
        let fixture = AppFixture::with_pull_request();
        let mut app = AppFixture::showing(&fixture, "#142");

        AppFixture::press(&mut app, 'g');
        AppFixture::perform_key(&mut app, 'x');

        assert_eq!(
            fixture.web_browser().opened(),
            [ServiceFixture::pull_request_url(142)]
        );
    }

    #[test]
    fn o_on_a_session_without_a_known_pull_request_says_so_and_opens_nothing() {
        let fixture = AppFixture::deletable();
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::perform_key(&mut app, 'o');

        assert_eq!(
            app.status.message(),
            Some(&StatusMessage::Error(
                "no pull request is known for fix-auth".to_string()
            ))
        );
        assert!(fixture.web_browser().opened().is_empty());
    }

    #[test]
    fn a_browser_that_cannot_open_shows_why_as_an_error() {
        let missing = WebBrowserError::NoDisplay;
        let fixture = AppFixture::with_pull_request()
            .with_web_browser(FakeWebBrowser::failing(missing.clone()));
        let mut app = AppFixture::showing(&fixture, "#142");

        AppFixture::perform_key(&mut app, 'o');

        assert_eq!(
            app.status.message(),
            Some(&StatusMessage::Error(missing.to_string()))
        );
    }

    #[test]
    fn returning_from_an_attach_looks_up_the_pull_requests_again() {
        let fixture = AppFixture::with_pull_request();
        let mut app = AppFixture::showing(&fixture, "#142");

        app.attach("fix-auth");

        assert!(AppFixture::looks_up_pull_requests_again(&fixture, &mut app));
    }

    #[test]
    fn returning_from_a_shell_window_looks_up_the_pull_requests_again() {
        let fixture = AppFixture::with_pull_request();
        let mut app = AppFixture::showing(&fixture, "#142");

        app.open_shell("fix-auth");

        assert!(AppFixture::looks_up_pull_requests_again(&fixture, &mut app));
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
        UiHarness::settle(&mut app);
        let after_the_timeout = AppFixture::observe_calls(&fixture);
        app.on_tick(start + Duration::from_secs(4));
        UiHarness::settle(&mut app);
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
        UiHarness::settle(&mut app);

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
    fn each_tick_turns_the_spinner_of_a_working_session() {
        let fixture = AppFixture::two_working_sessions_that_ring();
        let mut app = AppFixture::started(&fixture, 100, 28);
        let before = AppFixture::spinner(&AppFixture::screen(&app, 100, 28));

        app.on_tick(Instant::now());

        let after = AppFixture::spinner(&AppFixture::screen(&app, 100, 28));
        assert!(before.is_some());
        assert_ne!(after, before);
    }

    #[test]
    fn u_opens_the_usage_view_and_closes_it_again() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, 'u');
        let opened = AppFixture::screen(&app, 100, 28);
        AppFixture::press(&mut app, 'u');

        assert!(opened.contains("┌ Usage "));
        assert!(!app.dialogs.is_open());
    }

    #[test]
    fn the_open_usage_view_measures_the_agents() {
        let fixture = AppFixture::measuring_usage();
        let mut app = AppFixture::started(&fixture, 100, 28);

        AppFixture::press(&mut app, 'u');
        let shown = Deadline::for_background_work().wait_until(|| {
            app.on_tick(Instant::now());
            AppFixture::screen(&app, 100, 28).contains("Current session")
        });

        assert!(shown);
    }

    #[test]
    fn a_tick_with_the_usage_view_closed_measures_nothing() {
        let fixture = AppFixture::measuring_usage();
        let mut app = AppFixture::started(&fixture, 100, 28);

        app.on_tick(Instant::now());
        UiHarness::settle(&mut app);

        assert!(fixture.usage_meter().calls().is_empty());
    }

    #[test]
    fn a_startup_note_without_warnings_shows_as_information() {
        let fixture = AppFixture::two_waiting_sessions();

        let app = AppFixture::started(&fixture, 100, 28)
            .with_startup_messages(&[], Some("recreated the socket".to_string()));

        assert_eq!(
            app.status.message(),
            Some(&StatusMessage::Info("recreated the socket".to_string()))
        );
    }

    #[test]
    fn a_startup_note_follows_the_warnings_it_comes_with() {
        let fixture = AppFixture::two_waiting_sessions();
        let warnings = ["the config failed".to_string()];

        let app = AppFixture::started(&fixture, 100, 28)
            .with_startup_messages(&warnings, Some("recreated the socket".to_string()));

        assert_eq!(
            app.status.message(),
            Some(&StatusMessage::Error(
                "the config failed; recreated the socket".to_string()
            ))
        );
    }

    #[test]
    fn the_help_shows_the_log_file_of_the_diagnostic_paths() {
        let fixture = AppFixture::log_file_outside_the_state_directory();
        let mut app = AppFixture::started(&fixture, 100, 28);

        let help = AppFixture::help_scrolled_past_its_end(&mut app, 100, 28);

        assert!(help.contains(&format!(
            "Log          {}",
            AppFixture::LOG_FILE_OUTSIDE_THE_STATE_DIRECTORY
        )));
    }

    #[test]
    fn slash_then_enter_asks_to_attach_the_typed_session_and_closes_the_finder() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);
        AppFixture::press(&mut app, '/');
        AppFixture::press(&mut app, 'f');
        AppFixture::press(&mut app, 'a');

        let request = app.handle_event(&UiHarness::key(KeyCode::Enter));

        assert_eq!(request, AppRequest::Attach("fix-auth".to_string()));
        assert!(!app.dialogs.is_open());
    }

    #[test]
    fn control_k_opens_the_session_finder() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);

        app.handle_event(&UiHarness::control('k'));

        assert!(AppFixture::screen(&app, 100, 28).contains("┌ Find session "));
    }

    #[test]
    fn control_q_closes_every_dialog() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);

        for opener in ['/', 'n', 'R', '?', 'u'] {
            AppFixture::press(&mut app, opener);
            let opened = app.dialogs.is_open();
            app.handle_event(&UiHarness::control('q'));

            assert!(opened, "{opener} opened no dialog");
            assert!(
                !app.dialogs.is_open(),
                "Ctrl-Q left the {opener} dialog open"
            );
        }
    }

    #[test]
    fn control_q_cancels_the_delete_confirmation() {
        let fixture = AppFixture::deletable();
        let mut app = AppFixture::started(&fixture, 100, 28);
        let measure = app.handle_event(&UiHarness::character('d'));
        AppFixture::perform(&mut app, measure);

        let request = app.handle_event(&UiHarness::control('q'));

        assert_eq!(request, AppRequest::Continue);
        assert!(!app.dialogs.is_open());
        assert_eq!(fixture.registry().sessions().len(), 1);
    }

    #[test]
    fn control_q_on_the_home_screen_does_not_quit() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);

        let request = app.handle_event(&UiHarness::control('q'));

        assert_eq!(request, AppRequest::Continue);
    }

    #[test]
    fn attaching_selects_the_attached_session() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28);

        app.attach("fix-auth");

        assert_eq!(AppFixture::selected(&app).as_deref(), Some("fix-auth"));
    }

    #[test]
    fn a_key_press_clears_a_shown_message() {
        let fixture = AppFixture::two_waiting_sessions();
        let mut app = AppFixture::started(&fixture, 100, 28)
            .with_startup_messages(&["the config failed".to_string()], None);

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
        UiHarness::settle(&mut app);

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
        UiHarness::settle(&mut app);

        assert_eq!(fixture.registry().save_count(), 1);
    }
}

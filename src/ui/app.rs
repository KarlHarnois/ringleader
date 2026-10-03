use std::time::Instant;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::Frame;

use super::home_view::{HomeView, LayoutTier};
use super::preview_panel::{PreviewCapture, PreviewPanel};
use super::session_table::SessionTable;
use super::status_bar::{StatusBar, StatusNote};
use super::theme::Theme;
use crate::application::{ReturnMode, SessionService};
use crate::domain::{SessionState, SessionSummary};

pub struct App {
    service: SessionService,
    rows: Vec<SessionSummary>,
    selection: Option<String>,
    message: Option<String>,
    preview: Option<PreviewCapture>,
    tier: LayoutTier,
    tick: u64,
    next_observe: Instant,
    next_preview: Instant,
    theme: Theme,
}

impl App {
    const STOPPED: &'static str = "the tmux session is stopped";

    pub fn new(service: SessionService, now: Instant) -> Self {
        Self {
            service,
            rows: Vec::new(),
            selection: None,
            message: None,
            preview: None,
            tier: LayoutTier::TooSmall,
            tick: 0,
            next_observe: now,
            next_preview: now,
            theme: Theme,
        }
    }

    pub fn with_message(self, message: Option<String>) -> Self {
        Self { message, ..self }
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
        self.tier = LayoutTier::for_size(width, height);
        self.refresh_preview();
    }

    pub fn on_tick(&mut self, now: Instant) {
        self.tick = self.tick.wrapping_add(1);
        let intervals = self.service.config().intervals();
        if now >= self.next_observe {
            self.next_observe = now + intervals.state_poll();
            self.observe();
        }
        if now >= self.next_preview {
            self.next_preview = now + intervals.preview_refresh();
            self.refresh_preview();
        }
    }

    pub fn attach(&mut self, name: &str) {
        match self.service.attach(name, ReturnMode::None) {
            Ok(outcome) => self.message = outcome.binding_warning().map(str::to_string),
            Err(error) => self.message = Some(error.to_string()),
        }
        self.observe();
    }

    pub fn render(&self, frame: &mut Frame) {
        let selected = self.selected_row();
        let table = SessionTable::new(&self.rows, self.selection.as_deref(), self.tick, self.theme);
        let preview = PreviewPanel::new(selected, self.preview.as_ref(), self.theme);
        let prefix = self.service.config().naming().tmux_prefix();
        let note = selected.and_then(|row| StatusNote::new(row, prefix).text());
        let status = StatusBar::new(self.theme)
            .with_message(self.message.as_deref())
            .with_note(note);
        HomeView::new(table, preview, status).render(frame);
    }

    fn handle_key(&mut self, key: &KeyEvent) -> AppRequest {
        self.message = None;
        match key.code {
            KeyCode::Char('q') => AppRequest::Quit,
            KeyCode::Down | KeyCode::Right | KeyCode::Char('j' | 'l') => self.move_selection(1),
            KeyCode::Up | KeyCode::Left | KeyCode::Char('k' | 'h') => self.move_selection(-1),
            KeyCode::Enter => match &self.selection {
                Some(name) => AppRequest::Attach(name.clone()),
                None => AppRequest::Continue,
            },
            _ => AppRequest::Continue,
        }
    }

    fn move_selection(&mut self, offset: isize) -> AppRequest {
        let current = self.selected_index().unwrap_or(0);
        let last = self.rows.len().saturating_sub(1);
        let target = current.saturating_add_signed(offset).min(last);
        self.select(self.rows.get(target).map(|row| row.name().to_string()));
        AppRequest::Continue
    }

    fn select(&mut self, selection: Option<String>) {
        if selection == self.selection {
            return;
        }
        self.selection = selection;
        self.refresh_preview();
    }

    fn selected_index(&self) -> Option<usize> {
        let selected = self.selection.as_deref()?;
        self.rows.iter().position(|row| row.name() == selected)
    }

    fn selected_row(&self) -> Option<&SessionSummary> {
        self.rows.get(self.selected_index()?)
    }

    fn refresh_preview(&mut self) {
        self.preview = self
            .selected_row()
            .filter(|_| self.tier.shows_preview())
            .map(|row| self.capture(row));
    }

    fn capture(&self, row: &SessionSummary) -> PreviewCapture {
        if row.state() == SessionState::Stopped {
            return PreviewCapture::Unavailable(Self::STOPPED.to_string());
        }
        match self.service.preview(row.tmux_name()) {
            Ok(text) => PreviewCapture::Text(text),
            Err(error) => PreviewCapture::Unavailable(error.to_string()),
        }
    }

    fn observe(&mut self) {
        let persist = self.service.read_only_reason().is_none();
        match self.service.observe(persist) {
            Ok(rows) => self.show(rows),
            Err(error) => self.message = Some(error.to_string()),
        }
    }

    fn show(&mut self, mut rows: Vec<SessionSummary>) {
        rows.sort_by(|first, second| {
            (first.state(), first.name()).cmp(&(second.state(), second.name()))
        });
        self.rows = rows;
        if self.selected_index().is_none() {
            self.select(self.rows.first().map(|row| row.name().to_string()));
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppRequest {
    Continue,
    Redraw,
    Quit,
    Attach(String),
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
            app.selection.clone()
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
    fn rows_sort_by_state_priority_then_name() {
        let waiting = ServiceFixture::session("a-waiting");
        let working = ServiceFixture::session("b-working");
        let stopped = ServiceFixture::session("c-stopped");
        let fixture = ServiceFixture::new()
            .with_sessions(&[waiting.clone(), working.clone(), stopped.clone()])
            .with_multiplexer(
                FakeMultiplexer::new()
                    .with_live(ServiceFixture::waiting(&waiting))
                    .with_live(ServiceFixture::working(&working)),
            );

        let app = AppFixture::started(&fixture, 100, 28);
        let names: Vec<&str> = app.rows.iter().map(SessionSummary::name).collect();

        assert_eq!(names, ["b-working", "a-waiting", "c-stopped"]);
    }

    #[test]
    fn the_selection_follows_its_session_when_rows_reorder() {
        let fixture = AppFixture::two_waiting_sessions();
        let start = Instant::now();
        let mut app = AppFixture::started(&fixture, 100, 28);
        AppFixture::press(&mut app, 'j');
        fixture.multiplexer().end_session("ringleader-dark-mode");

        app.on_tick(start + Duration::from_secs(2));

        assert_eq!(app.rows.first().unwrap().name(), "fix-auth");
        assert_eq!(AppFixture::selected(&app).as_deref(), Some("fix-auth"));
    }

    #[test]
    fn the_status_bar_explains_ctrl_q_for_a_session_under_an_earlier_prefix() {
        let session = ServiceFixture::session("fix-auth");
        let draft = ConfigDraft::new(Path::new(ServiceFixture::WORKTREE_ROOT))
            .tmux_prefix("agents-")
            .agent(ServiceFixture::agent());
        let fixture = ServiceFixture::new()
            .with_config(Config::new(&draft).unwrap())
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

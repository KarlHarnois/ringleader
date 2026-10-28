use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyEvent, KeyEventKind};
use ratatui::backend::Backend;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Padding, Paragraph};
use ratatui::{Frame, Terminal};
use thiserror::Error;

use super::dialogs::DialogStep;
use super::dialogs::usage_view::UsageView;
use super::event_loop::InputSource;
use super::session_menu::{MenuItem, SessionMenu};
use super::theme::{ColorMode, Theme};
use crate::application::{CorruptRegistry, ServiceError, SessionService};

pub struct SessionMenuPopup {
    service: SessionService,
    client: String,
    theme: Theme,
    color_mode: ColorMode,
}

impl SessionMenuPopup {
    const TICK: Duration = Duration::from_millis(100);

    pub fn new(service: SessionService, client: &str) -> Self {
        Self {
            service,
            client: client.to_string(),
            theme: Theme::default(),
            color_mode: ColorMode::default(),
        }
    }

    pub fn with_color_mode(self, color_mode: ColorMode) -> Self {
        Self { color_mode, ..self }
    }

    pub fn run<B, I>(
        mut self,
        terminal: &mut Terminal<B>,
        mut input: I,
    ) -> Result<(), MenuPopupError>
    where
        B: Backend,
        B::Error: Send + Sync + 'static,
        I: InputSource,
    {
        self.load_theme();
        self.service.load_registry(CorruptRegistry::Report)?;
        let name = self.service.client_session(&self.client)?;
        let mut panel = Panel::Menu(SessionMenu::open(&name, &self.service));
        loop {
            terminal
                .draw(|frame| panel.render(frame, &self.theme))
                .map_err(io::Error::other)?;
            let event = input.next_event(Self::TICK)?;
            panel.poll(Instant::now(), &self.service);
            let Some(key) = event.as_ref().and_then(Self::pressed) else {
                continue;
            };
            panel = match self.answer(panel, key) {
                Some(next) => next,
                None => return Ok(()),
            };
        }
    }

    fn load_theme(&mut self) {
        self.service.load_theme();
        self.theme = self.color_mode.theme(self.service.theme());
    }

    fn pressed(event: &Event) -> Option<&KeyEvent> {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => Some(key),
            _ => None,
        }
    }

    fn answer(&mut self, panel: Panel, key: &KeyEvent) -> Option<Panel> {
        match panel {
            Panel::Menu(menu) => self.answer_menu(menu, key),
            Panel::Usage(mut view) => match view.handle_key(key) {
                DialogStep::Stay => Some(Panel::Usage(view)),
                DialogStep::Close | DialogStep::Request(_) | DialogStep::CloseWith(_) => None,
            },
        }
    }

    fn answer_menu(&mut self, mut menu: SessionMenu, key: &KeyEvent) -> Option<Panel> {
        let item = MenuItem::for_key(key).filter(|_| !menu.shows_failure())?;
        match self.perform(item, menu.name()) {
            Ok(next) => next,
            Err(error) => {
                menu.fail(&error);
                Some(Panel::Menu(menu))
            }
        }
    }

    fn perform(&mut self, item: MenuItem, name: &str) -> Result<Option<Panel>, ServiceError> {
        let client = &self.client;
        let outcome = match item {
            MenuItem::Usage => return Ok(Some(Panel::usage(&self.service))),
            MenuItem::PullRequest => self.service.open_pull_request(name),
            MenuItem::Terminal => self.service.open_terminal(name),
            MenuItem::ShellToggle => self.service.toggle_shell(client),
            MenuItem::NewSession => self.service.new_session_on_return(client),
            MenuItem::Delete => self.service.delete_on_return(client),
            MenuItem::Return => self.service.return_client(client),
        };
        outcome.map(|()| None)
    }
}

enum Panel {
    Menu(SessionMenu),
    Usage(Box<UsageView>),
}

impl Panel {
    const TITLE_INDENT: u16 = 1;

    fn usage(service: &SessionService) -> Self {
        Self::Usage(Box::new(UsageView::open(service)))
    }

    fn poll(&mut self, now: Instant, service: &SessionService) {
        match self {
            Self::Menu(menu) => menu.poll(service),
            Self::Usage(view) => view.poll(now, service),
        }
    }

    fn render(&self, frame: &mut Frame, theme: &Theme) {
        let block = Block::default()
            .borders(Borders::TOP)
            .title(self.title())
            .padding(Padding::left(Self::TITLE_INDENT));
        let inner = block.inner(frame.area());
        frame.render_widget(block, frame.area());
        frame.render_widget(Paragraph::new(self.lines(inner, theme)), inner);
    }

    fn title(&self) -> String {
        match self {
            Self::Menu(menu) => format!(" {} ", menu.name()),
            Self::Usage(_) => UsageView::TITLE.to_string(),
        }
    }

    fn lines(&self, area: Rect, theme: &Theme) -> Vec<Line<'static>> {
        match self {
            Self::Menu(menu) => menu.lines(theme),
            Self::Usage(view) => {
                view.lines(usize::from(area.width), usize::from(area.height), theme)
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum MenuPopupError {
    #[error("the session menu lost its terminal: {0}")]
    Terminal(#[from] io::Error),
    #[error(transparent)]
    Service(#[from] ServiceError),
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crossterm::event::{KeyCode, KeyModifiers};

    use super::*;
    use crate::application::ports::{AttachTarget, ClientView, ReturnRequest};
    use crate::domain::PullRequestState;
    use crate::test_support::{
        FakeMultiplexer, FixtureBackend, LogCapture, ScriptedInput, ServiceFixture, UiHarness,
    };

    struct PopupFixture {
        frames: Vec<String>,
    }

    impl PopupFixture {
        const CLIENT: &'static str = "client-1234";
        const NAME: &'static str = "fix-auth";
        const TMUX_NAME: &'static str = "ringleader-fix-auth";
        const WIDTH: u16 = 80;
        const ROWS: u16 = 6;

        fn run(fixture: &ServiceFixture, events: Vec<Event>) -> Result<Self, MenuPopupError> {
            let mut terminal = Terminal::new(FixtureBackend::new(Self::WIDTH, Self::ROWS))?;
            SessionMenuPopup::new(fixture.service(), Self::CLIENT)
                .run(&mut terminal, ScriptedInput::events(events))?;
            let frames = terminal.backend().frames().to_vec();
            Ok(Self { frames })
        }

        fn pressing(fixture: &ServiceFixture, keys: &str) -> Self {
            let events = keys.chars().map(UiHarness::character).collect();
            Self::run(fixture, events).unwrap()
        }

        fn shows(&self, text: &str) -> bool {
            self.frames.iter().any(|frame| frame.contains(text))
        }

        fn attached() -> ServiceFixture {
            let session = ServiceFixture::session(Self::NAME);
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_host(ServiceFixture::host_for(&[&session]))
                .with_multiplexer(
                    FakeMultiplexer::new()
                        .with_live(ServiceFixture::waiting(&session))
                        .with_client_view(Self::CLIENT, ClientView::showing_agent(Self::TMUX_NAME)),
                )
        }

        fn with_pull_request() -> ServiceFixture {
            let fixture = Self::attached();
            fixture.pull_requests().add_pull_request(
                Path::new(ServiceFixture::REPO),
                ServiceFixture::session(Self::NAME).branch(),
                ServiceFixture::pull_request(142, PullRequestState::Open),
            );
            fixture
        }

        fn escape() -> Event {
            UiHarness::key(KeyCode::Esc)
        }

        fn control_space() -> Event {
            Event::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL))
        }
    }

    #[test]
    fn the_panel_shows_the_session_and_its_items() {
        let fixture = PopupFixture::attached();

        let popup = PopupFixture::run(&fixture, vec![PopupFixture::escape()]).unwrap();

        assert!(popup.shows(" fix-auth "));
        assert!(popup.shows(" n  new session… (in Ringleader)   u  usage"));
    }

    #[test]
    fn o_opens_the_pull_request_of_the_session() {
        let fixture = PopupFixture::with_pull_request();

        PopupFixture::pressing(&fixture, "o");

        assert_eq!(
            fixture.web_browser().opened(),
            [ServiceFixture::pull_request_url(142)]
        );
    }

    #[test]
    fn o_without_a_pull_request_shows_the_refusal_until_the_next_key() {
        let fixture = PopupFixture::attached();

        let popup = PopupFixture::pressing(&fixture, "ox");

        assert!(popup.shows("no pull request is known for fix-auth"));
    }

    #[test]
    fn a_refusal_is_logged() {
        let log = LogCapture::installed();
        let fixture = PopupFixture::attached();
        let before = log.count_containing("the session menu of fix-auth failed");

        PopupFixture::pressing(&fixture, "ox");

        assert!(log.count_containing("the session menu of fix-auth failed") > before);
    }

    #[test]
    fn capital_t_opens_a_terminal_window_in_the_worktree() {
        let fixture = PopupFixture::attached();

        PopupFixture::pressing(&fixture, "T");

        assert_eq!(fixture.terminal_emulator().opened().len(), 1);
    }

    #[test]
    fn t_toggles_the_client_to_the_shell_window() {
        let fixture = PopupFixture::attached();

        PopupFixture::pressing(&fixture, "t");

        assert_eq!(
            fixture.multiplexer().switched(),
            [(
                PopupFixture::CLIENT.to_string(),
                AttachTarget::Shell("@9".to_string())
            )]
        );
    }

    #[test]
    fn d_hands_the_delete_to_ringleader_and_returns_the_client() {
        let fixture = PopupFixture::attached();

        PopupFixture::pressing(&fixture, "d");

        assert_eq!(
            fixture.multiplexer().recorded_return_request(),
            Some(ReturnRequest::Delete(PopupFixture::TMUX_NAME.to_string()))
        );
        assert_eq!(fixture.multiplexer().detached(), [PopupFixture::CLIENT]);
    }

    #[test]
    fn n_hands_a_new_session_in_the_repo_to_ringleader_and_returns_the_client() {
        let fixture = PopupFixture::attached();

        PopupFixture::pressing(&fixture, "n");

        assert_eq!(
            fixture.multiplexer().recorded_return_request(),
            Some(ReturnRequest::NewSession(Some(PathBuf::from(
                ServiceFixture::REPO
            ))))
        );
        assert_eq!(fixture.multiplexer().detached(), [PopupFixture::CLIENT]);
    }

    #[test]
    fn q_returns_the_client_to_ringleader() {
        let fixture = PopupFixture::attached();

        PopupFixture::pressing(&fixture, "q");

        assert_eq!(fixture.multiplexer().detached(), [PopupFixture::CLIENT]);
    }

    #[test]
    fn u_swaps_the_menu_for_the_usage_view() {
        let fixture = PopupFixture::attached();

        let popup = PopupFixture::pressing(&fixture, "uu");

        assert!(popup.shows(UsageView::TITLE));
    }

    #[test]
    fn the_usage_view_starts_in_the_column_of_its_title_like_the_menu() {
        let fixture = PopupFixture::attached();

        let popup = PopupFixture::pressing(&fixture, "uu");

        assert!(popup.shows("\n Measuring usage…"));
    }

    #[test]
    fn escape_control_space_and_unbound_keys_close_the_panel_without_an_action() {
        for key in [
            PopupFixture::escape(),
            PopupFixture::control_space(),
            UiHarness::character('x'),
        ] {
            let fixture = PopupFixture::attached();

            PopupFixture::run(&fixture, vec![key.clone()]).unwrap();

            let multiplexer = fixture.multiplexer();
            assert!(multiplexer.state_changing_calls().is_empty(), "{key:?}");
            assert!(multiplexer.switched().is_empty(), "{key:?}");
        }
    }

    #[test]
    fn a_quiet_tick_keeps_the_panel_open() {
        let fixture = PopupFixture::attached();
        let input =
            ScriptedInput::events(Vec::new()).then_after_a_pause(vec![UiHarness::character('q')]);
        let mut terminal =
            Terminal::new(FixtureBackend::new(PopupFixture::WIDTH, PopupFixture::ROWS)).unwrap();

        SessionMenuPopup::new(fixture.service(), PopupFixture::CLIENT)
            .run(&mut terminal, input)
            .unwrap();

        assert_eq!(fixture.multiplexer().detached(), [PopupFixture::CLIENT]);
    }

    #[test]
    fn the_popup_takes_no_lock_so_it_never_writes_the_registry() {
        let fixture = PopupFixture::attached();

        PopupFixture::run(&fixture, vec![PopupFixture::escape()]).unwrap();

        assert_eq!(fixture.registry().save_count(), 0);
    }

    #[test]
    fn a_client_that_shows_no_session_is_returned_as_not_found() {
        let fixture = ServiceFixture::new();

        let outcome = PopupFixture::run(&fixture, vec![PopupFixture::escape()]);

        assert!(matches!(
            outcome,
            Err(MenuPopupError::Service(ServiceError::NotFound(_)))
        ));
    }

    #[test]
    fn a_lost_terminal_is_returned() {
        let fixture = PopupFixture::attached();
        let mut terminal =
            Terminal::new(FixtureBackend::new(PopupFixture::WIDTH, PopupFixture::ROWS)).unwrap();

        let outcome = SessionMenuPopup::new(fixture.service(), PopupFixture::CLIENT)
            .run(&mut terminal, ScriptedInput::failing());

        assert!(matches!(outcome, Err(MenuPopupError::Terminal(_))));
    }
}

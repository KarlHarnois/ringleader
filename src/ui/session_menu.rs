use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::theme::Theme;
use crate::application::{Pending, ServiceError, SessionService};
use crate::domain::PullRequest;

type PullRequestLookup = Pending<HashMap<String, Option<PullRequest>>>;

pub struct SessionMenu {
    name: String,
    lookup: Option<PullRequestLookup>,
    has_pull_request: bool,
    failure: Option<String>,
}

impl SessionMenu {
    const KEY_GAP: &'static str = "  ";
    const COLUMN_GAP: &'static str = "   ";

    pub fn open(name: &str, service: &SessionService) -> Self {
        Self {
            name: name.to_string(),
            lookup: Some(service.find_pull_requests(&[name.to_string()])),
            has_pull_request: service.latest_pull_request(name).is_some(),
            failure: None,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn shows_failure(&self) -> bool {
        self.failure.is_some()
    }

    pub fn poll(&mut self, service: &SessionService) {
        let Some(finished) = self.lookup.as_ref().and_then(Pending::poll) else {
            return;
        };
        self.lookup = None;
        match finished {
            Ok(_) => self.has_pull_request = service.latest_pull_request(&self.name).is_some(),
            Err(error) => log::warn!(
                "could not look up the pull request of {}: {error}",
                self.name
            ),
        }
    }

    pub fn fail(&mut self, error: &ServiceError) {
        log::warn!("the session menu of {} failed: {error}", self.name);
        self.failure = Some(error.to_string());
    }

    pub fn lines(&self, theme: &Theme) -> Vec<Line<'static>> {
        let widths = MenuItem::COLUMNS.map(Self::description_width);
        let mut lines: Vec<Line<'static>> = (0..MenuItem::rows())
            .map(|row| self.row(row, widths, theme))
            .collect();
        lines.push(self.status_line(theme));
        lines
    }

    fn description_width(column: &[MenuItem]) -> usize {
        column
            .iter()
            .map(|item| item.description().chars().count())
            .max()
            .unwrap_or_default()
    }

    fn row(&self, row: usize, widths: [usize; 2], theme: &Theme) -> Line<'static> {
        let spans = MenuItem::COLUMNS
            .iter()
            .zip(widths)
            .filter_map(|(column, width)| column.get(row).map(|item| (*item, width)))
            .flat_map(|(item, width)| self.cell(item, width, theme));
        Line::from(spans.collect::<Vec<_>>())
    }

    fn cell(&self, item: MenuItem, width: usize, theme: &Theme) -> [Span<'static>; 2] {
        let (key_style, description_style) = self.styles(item, theme);
        [
            Span::styled(item.key().to_string(), key_style),
            Span::styled(
                format!(
                    "{}{:<width$}{}",
                    Self::KEY_GAP,
                    item.description(),
                    Self::COLUMN_GAP
                ),
                description_style,
            ),
        ]
    }

    fn styles(&self, item: MenuItem, theme: &Theme) -> (Style, Style) {
        match item == MenuItem::PullRequest && !self.has_pull_request {
            true => (theme.dim(), theme.dim()),
            false => (theme.accent(), theme.base()),
        }
    }

    fn status_line(&self, theme: &Theme) -> Line<'static> {
        match &self.failure {
            Some(failure) => Line::styled(failure.clone(), theme.error()),
            None => Line::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    NewSession,
    PullRequest,
    Terminal,
    ShellToggle,
    Usage,
    Delete,
    Return,
}

impl MenuItem {
    const COLUMNS: [&'static [Self]; 2] = [
        &[
            Self::NewSession,
            Self::PullRequest,
            Self::Terminal,
            Self::ShellToggle,
        ],
        &[Self::Usage, Self::Delete, Self::Return],
    ];

    pub fn for_key(key: &KeyEvent) -> Option<Self> {
        let KeyCode::Char(character) = key.code else {
            return None;
        };
        let typed = KeyModifiers::SHIFT.contains(key.modifiers);
        Self::COLUMNS
            .iter()
            .flat_map(|column| column.iter().copied())
            .find(|item| item.key() == character)
            .filter(|_| typed)
    }

    fn rows() -> usize {
        Self::COLUMNS
            .iter()
            .map(|column| column.len())
            .max()
            .unwrap_or_default()
    }

    fn key(self) -> char {
        match self {
            Self::NewSession => 'n',
            Self::PullRequest => 'o',
            Self::Terminal => 'T',
            Self::ShellToggle => 't',
            Self::Usage => 'u',
            Self::Delete => 'd',
            Self::Return => 'q',
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::NewSession => "new session… (in Ringleader)",
            Self::PullRequest => "open pull request",
            Self::Terminal => "open terminal window",
            Self::ShellToggle => "toggle agent / shell",
            Self::Usage => "usage",
            Self::Delete => "delete session… (in Ringleader)",
            Self::Return => "return to Ringleader",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ratatui::style::Modifier;

    use super::*;
    use crate::domain::PullRequestState;
    use crate::test_support::{Deadline, ServiceFixture};

    struct MenuFixture;

    impl MenuFixture {
        const NAME: &'static str = "fix-auth";

        fn with_pull_request() -> ServiceFixture {
            let fixture = Self::without_pull_request();
            fixture.pull_requests().add_pull_request(
                Path::new(ServiceFixture::REPO),
                ServiceFixture::session(Self::NAME).branch(),
                ServiceFixture::pull_request(142, PullRequestState::Open),
            );
            fixture
        }

        fn without_pull_request() -> ServiceFixture {
            ServiceFixture::new().with_sessions(&[ServiceFixture::session(Self::NAME)])
        }

        fn looked_up(fixture: &ServiceFixture) -> (SessionMenu, SessionService) {
            let service = fixture.service();
            let mut menu = SessionMenu::open(Self::NAME, &service);
            let finished = Deadline::for_background_work().wait_until(|| {
                menu.poll(&service);
                menu.lookup.is_none()
            });
            assert!(finished, "the pull request lookup never finished");
            (menu, service)
        }

        fn text(lines: &[Line]) -> Vec<String> {
            lines
                .iter()
                .map(|line| line.to_string().trim_end().to_string())
                .collect()
        }

        fn style_of(lines: &[Line], content: &str) -> Style {
            lines
                .iter()
                .flat_map(|line| line.spans.iter())
                .find(|span| span.content == content)
                .map(|span| span.style)
                .unwrap()
        }

        fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
            KeyEvent::new(code, modifiers)
        }
    }

    #[test]
    fn the_panel_lists_keys_left_of_their_descriptions_in_two_columns() {
        let (menu, _service) = MenuFixture::looked_up(&MenuFixture::with_pull_request());

        let text = MenuFixture::text(&menu.lines(&Theme));

        assert_eq!(
            text,
            [
                "n  new session… (in Ringleader)   u  usage",
                "o  open pull request              d  delete session… (in Ringleader)",
                "T  open terminal window           q  return to Ringleader",
                "t  toggle agent / shell",
                "",
            ]
        );
    }

    #[test]
    fn keys_show_in_the_accent_color() {
        let (menu, _service) = MenuFixture::looked_up(&MenuFixture::with_pull_request());

        let style = MenuFixture::style_of(&menu.lines(&Theme), "T");

        assert_eq!(style, Theme.accent());
    }

    #[test]
    fn a_session_with_a_pull_request_shows_its_open_key_undimmed() {
        let (menu, _service) = MenuFixture::looked_up(&MenuFixture::with_pull_request());

        let style = MenuFixture::style_of(&menu.lines(&Theme), "o");

        assert!(!style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn a_session_without_a_pull_request_dims_its_open_key() {
        let (menu, _service) = MenuFixture::looked_up(&MenuFixture::without_pull_request());

        let style = MenuFixture::style_of(&menu.lines(&Theme), "o");

        assert!(style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn a_failure_shows_on_the_last_line() {
        let (mut menu, _service) = MenuFixture::looked_up(&MenuFixture::with_pull_request());

        menu.fail(&ServiceError::NotFound(
            "no terminal emulator found".to_string(),
        ));

        let text = MenuFixture::text(&menu.lines(&Theme));
        assert_eq!(text.last().unwrap(), "no terminal emulator found");
    }

    #[test]
    fn a_shifted_letter_picks_its_capital_item() {
        let key = MenuFixture::key(KeyCode::Char('T'), KeyModifiers::SHIFT);

        assert_eq!(MenuItem::for_key(&key), Some(MenuItem::Terminal));
    }

    #[test]
    fn a_control_chord_picks_no_item() {
        let key = MenuFixture::key(KeyCode::Char('o'), KeyModifiers::CONTROL);

        assert_eq!(MenuItem::for_key(&key), None);
    }

    #[test]
    fn a_letter_without_an_item_picks_none() {
        let key = MenuFixture::key(KeyCode::Char('x'), KeyModifiers::NONE);

        assert_eq!(MenuItem::for_key(&key), None);
    }
}

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::Line;

use super::dialog::DialogStep;
use super::fuzzy_ranker::FuzzyRanker;
use super::match_list::MatchList;
use super::search_query::SearchQuery;
use super::text_field::FieldKind;
use crate::ui::app::AppRequest;
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct SessionFinder {
    query: SearchQuery,
    names: Vec<String>,
    ranker: FuzzyRanker,
    matches: MatchList,
}

impl SessionFinder {
    pub const TITLE: &'static str = " Find session ";
    const HINTS: &'static str = "Enter attach · Esc cancel · ↑ ↓ ctrl-j ctrl-k choose";

    pub fn open(names: Vec<String>) -> Self {
        let mut finder = Self {
            query: SearchQuery::new(FieldKind::Name),
            names,
            ranker: FuzzyRanker::for_names(),
            matches: MatchList::default(),
        };
        finder.rank();
        finder
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> DialogStep {
        match key.code {
            KeyCode::Esc => DialogStep::Close,
            KeyCode::Enter => self.attach(),
            _ => {
                self.choose_or_edit(key);
                DialogStep::Stay
            }
        }
    }

    pub fn paste(&mut self, text: &str) {
        if self.query.paste(text).is_ok() {
            self.rank();
        }
    }

    pub fn lines(&self, width: usize, height: usize, theme: &Theme) -> Vec<Line<'static>> {
        let hints = TextFit::new(Self::HINTS).styled_lines(width, theme.dim());
        let mut lines = vec![
            self.query.line(width),
            Line::styled(self.count(), theme.dim()),
        ];
        let rows = height.saturating_sub(lines.len() + hints.len()).max(1);
        lines.extend(self.match_rows(width, rows, theme));
        lines.extend(hints);
        lines
    }

    fn attach(&self) -> DialogStep {
        match self.matches.selected() {
            Some(index) => DialogStep::CloseWith(AppRequest::Attach(self.names[index].clone())),
            None => DialogStep::Stay,
        }
    }

    fn choose_or_edit(&mut self, key: &KeyEvent) {
        if self.matches.move_cursor(key) {
            return;
        }
        if self.query.edit(key) {
            self.rank();
        }
    }

    fn rank(&mut self) {
        let ranked = self.ranker.ranked(self.query.value(), &self.names);
        self.matches.replace(ranked);
    }

    fn count(&self) -> String {
        format!("{}/{} sessions", self.matches.len(), self.names.len())
    }

    fn match_rows(&self, width: usize, rows: usize, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = self.matches.lines(rows, theme, |index| {
            TextFit::new(&self.names[index]).right_truncated(width)
        });
        lines.resize(rows, Line::default());
        lines
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::ui::dialogs::Dialog;

    struct FinderFixture;

    impl FinderFixture {
        const WIDTH: usize = 74;
        const HEIGHT: usize = 8;
        const SMALLEST_SCREEN: (u16, u16) = (36, 8);

        fn names() -> Vec<String> {
            let names = ["fix-auth", "dark-mode", "docs"];
            names.iter().map(|name| name.to_string()).collect()
        }

        fn finder() -> SessionFinder {
            SessionFinder::open(Self::names())
        }

        fn screen((width, height): (u16, u16)) -> String {
            let dialog = Dialog::find_session(Self::names());
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| dialog.render(frame, &Theme)).unwrap();
            let cells = terminal.backend().buffer().content();
            cells.iter().map(|cell| cell.symbol()).collect()
        }

        fn press(finder: &mut SessionFinder, code: KeyCode, modifiers: KeyModifiers) -> DialogStep {
            finder.handle_key(&KeyEvent::new(code, modifiers))
        }

        fn enter(finder: &mut SessionFinder) -> DialogStep {
            Self::press(finder, KeyCode::Enter, KeyModifiers::NONE)
        }

        fn type_text(finder: &mut SessionFinder, text: &str) {
            for character in text.chars() {
                Self::press(finder, KeyCode::Char(character), KeyModifiers::NONE);
            }
        }

        fn rows(finder: &SessionFinder) -> Vec<String> {
            finder
                .lines(Self::WIDTH, Self::HEIGHT, &Theme)
                .iter()
                .map(ToString::to_string)
                .collect()
        }

        fn attach(name: &str) -> DialogStep {
            DialogStep::CloseWith(AppRequest::Attach(name.to_string()))
        }
    }

    #[test]
    fn every_session_is_listed_before_any_typing() {
        let rows = FinderFixture::rows(&FinderFixture::finder());

        assert_eq!(
            rows[..5],
            ["Find: █", "3/3 sessions", "fix-auth", "dark-mode", "docs"]
        );
    }

    #[test]
    fn typing_filters_the_list_and_updates_the_count() {
        let mut finder = FinderFixture::finder();

        FinderFixture::type_text(&mut finder, "fa");

        assert_eq!(
            FinderFixture::rows(&finder)[..4],
            ["Find: fa█", "1/3 sessions", "fix-auth", ""]
        );
    }

    #[test]
    fn the_hints_sit_on_the_last_row() {
        let rows = FinderFixture::rows(&FinderFixture::finder());

        assert_eq!(rows.len(), FinderFixture::HEIGHT);
        assert_eq!(rows.last().unwrap(), SessionFinder::HINTS);
    }

    #[test]
    fn the_smallest_screen_keeps_enter_and_esc_in_view() {
        let screen = FinderFixture::screen(FinderFixture::SMALLEST_SCREEN);

        assert!(screen.contains("Enter attach · Esc cancel"), "{screen}");
    }

    #[test]
    fn enter_attaches_to_the_match_under_the_cursor() {
        let mut finder = FinderFixture::finder();

        assert_eq!(
            FinderFixture::enter(&mut finder),
            FinderFixture::attach("fix-auth")
        );
    }

    #[test]
    fn down_and_control_j_move_to_the_next_match() {
        let mut finder = FinderFixture::finder();

        FinderFixture::press(&mut finder, KeyCode::Down, KeyModifiers::NONE);
        FinderFixture::press(&mut finder, KeyCode::Char('j'), KeyModifiers::CONTROL);

        assert_eq!(
            FinderFixture::enter(&mut finder),
            FinderFixture::attach("docs")
        );
    }

    #[test]
    fn enter_without_a_match_keeps_the_finder_open() {
        let mut finder = FinderFixture::finder();
        FinderFixture::type_text(&mut finder, "zzz");

        assert_eq!(FinderFixture::enter(&mut finder), DialogStep::Stay);
    }

    #[test]
    fn esc_closes_the_finder() {
        let mut finder = FinderFixture::finder();

        assert_eq!(
            FinderFixture::press(&mut finder, KeyCode::Esc, KeyModifiers::NONE),
            DialogStep::Close
        );
    }

    #[test]
    fn j_and_k_are_typed_into_the_query() {
        let mut finder = FinderFixture::finder();

        FinderFixture::type_text(&mut finder, "jk");

        assert_eq!(FinderFixture::rows(&finder)[0], "Find: jk█");
    }

    #[test]
    fn a_pasted_query_filters_the_list() {
        let mut finder = FinderFixture::finder();

        finder.paste("docs");

        assert_eq!(
            FinderFixture::enter(&mut finder),
            FinderFixture::attach("docs")
        );
    }
}

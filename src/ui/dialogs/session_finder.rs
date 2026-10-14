use crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::{Line, Span};

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
    const PROMPT: &'static str = "❯ ";
    const RULE: char = '─';
    const COUNT_GAP: usize = 1;
    const SIDE_PADDING: usize = 1;

    pub fn open(names: Vec<String>) -> Self {
        let mut finder = Self {
            query: SearchQuery::new(FieldKind::Name, Self::PROMPT),
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
        let mut lines = vec![self.query_line(width, theme), Self::rule(width, theme)];
        let rows = height.saturating_sub(lines.len()).max(1);
        lines.extend(self.match_rows(width, rows, theme));
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

    fn query_line(&self, width: usize, theme: &Theme) -> Line<'static> {
        let count = self.count();
        let count_room = count.chars().count() + Self::SIDE_PADDING;
        let query_room = width.saturating_sub(count_room);
        let query = self.query.shown(query_room.saturating_sub(Self::COUNT_GAP));
        let padded_query = TextFit::new(&query).padded(query_room);
        let padded_count = TextFit::new(&count).padded(count_room);
        Line::from(vec![
            Span::raw(padded_query),
            Span::styled(padded_count, theme.dim()),
        ])
    }

    fn rule(width: usize, theme: &Theme) -> Line<'static> {
        Line::styled(Self::RULE.to_string().repeat(width), theme.dim())
    }

    fn count(&self) -> String {
        format!("{}/{}", self.matches.len(), self.names.len())
    }

    fn match_rows(&self, width: usize, rows: usize, theme: &Theme) -> Vec<Line<'static>> {
        let name_room = width.saturating_sub(Self::SIDE_PADDING);
        let indent = " ".repeat(Self::SIDE_PADDING);
        self.matches.lines(rows, theme, |index| {
            indent.clone() + &TextFit::new(&self.names[index]).right_truncated(name_room)
        })
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;

    use super::*;

    struct FinderFixture;

    impl FinderFixture {
        const WIDTH: usize = 20;
        const HEIGHT: usize = 8;

        fn names() -> Vec<String> {
            let names = ["fix-auth", "dark-mode", "docs"];
            names.iter().map(|name| name.to_string()).collect()
        }

        fn finder() -> SessionFinder {
            SessionFinder::open(Self::names())
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

        fn rule() -> String {
            "─".repeat(Self::WIDTH)
        }
    }

    #[test]
    fn every_session_is_listed_under_the_query_and_a_rule() {
        let rows = FinderFixture::rows(&FinderFixture::finder());

        assert_eq!(
            rows,
            [
                "❯ █             3/3 ".to_string(),
                FinderFixture::rule(),
                " fix-auth".to_string(),
                " dark-mode".to_string(),
                " docs".to_string(),
            ]
        );
    }

    #[test]
    fn typing_filters_the_list_and_updates_the_count() {
        let mut finder = FinderFixture::finder();

        FinderFixture::type_text(&mut finder, "fa");

        assert_eq!(
            FinderFixture::rows(&finder),
            [
                "❯ fa█           1/3 ".to_string(),
                FinderFixture::rule(),
                " fix-auth".to_string(),
            ]
        );
    }

    #[test]
    fn a_long_query_keeps_the_count_in_view() {
        let mut finder = FinderFixture::finder();

        FinderFixture::type_text(&mut finder, "abcdefghijklmnopqrstuvwxyz");

        assert_eq!(FinderFixture::rows(&finder)[0], "❯ opqrstuvwxyz█ 0/3 ");
    }

    #[test]
    fn a_long_name_is_truncated_after_its_indent() {
        let finder = SessionFinder::open(vec!["a-session-name-wider-than-the-finder".to_string()]);

        assert_eq!(FinderFixture::rows(&finder)[2], " a-session-name-wid…");
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

        assert!(FinderFixture::rows(&finder)[0].starts_with("❯ jk█"));
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

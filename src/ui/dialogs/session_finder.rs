use std::iter::once;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::{Line, Span};

use super::dialog::DialogStep;
use super::session_candidate::SessionCandidate;
use super::text_field::FieldKind;
use crate::domain::SessionSummary;
use crate::ui::app::AppRequest;
use crate::ui::dialogs::fuzzy_search::fuzzy_ranker::{FuzzyMatch, FuzzyRanker};
use crate::ui::dialogs::fuzzy_search::match_list::MatchList;
use crate::ui::dialogs::fuzzy_search::search_query::SearchQuery;
use crate::ui::table::session_tree::SessionTree;
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct SessionFinder {
    query: SearchQuery,
    candidates: Vec<SessionCandidate>,
    ranker: FuzzyRanker,
    matches: MatchList,
    error: Option<String>,
}

impl SessionFinder {
    pub const TITLE: &'static str = " Find session ";
    const PROMPT: &'static str = "❯ ";
    const RULE: char = '─';
    const COUNT_GAP: usize = 1;
    const SIDE_PADDING: usize = 1;

    pub fn open(rows: &[SessionSummary]) -> Self {
        let mut finder = Self {
            query: SearchQuery::new(FieldKind::Name, Self::PROMPT),
            candidates: Self::candidates(rows),
            ranker: FuzzyRanker::for_names(),
            matches: MatchList::default(),
            error: None,
        };
        finder.rank();
        finder
    }

    pub fn fail(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> DialogStep {
        self.error = None;
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
        self.error = None;
        if self.query.paste(text).is_ok() {
            self.rank();
        }
    }

    pub fn lines(&self, width: usize, height: usize, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = vec![self.query_line(width, theme), Self::rule(width, theme)];
        lines.extend(self.error_lines(width, theme));
        let rows = height.saturating_sub(lines.len()).max(1);
        lines.extend(self.match_rows(width, rows, theme));
        lines
    }

    fn candidates(rows: &[SessionSummary]) -> Vec<SessionCandidate> {
        let candidate = if SessionTree::groups_by_repo(rows) {
            SessionCandidate::prefixed
        } else {
            SessionCandidate::bare
        };
        rows.iter().map(candidate).collect()
    }

    fn attach(&self) -> DialogStep {
        match self.matches.selected() {
            Some(index) => {
                let name = self.candidates[index].name().to_string();
                DialogStep::CloseWith(AppRequest::Attach(name))
            }
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
        let shown = self.shown_candidates();
        let found = self.ranker.matches(self.query.value(), &shown);
        self.mark_matches(&found);
        self.matches
            .replace(found.iter().map(FuzzyMatch::index).collect());
    }

    fn mark_matches(&mut self, found: &[FuzzyMatch]) {
        for candidate in &mut self.candidates {
            candidate.mark(&[]);
        }
        for fuzzy_match in found {
            self.candidates[fuzzy_match.index()].mark(fuzzy_match.positions());
        }
    }

    fn shown_candidates(&self) -> Vec<String> {
        self.candidates
            .iter()
            .map(SessionCandidate::shown)
            .collect()
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

    fn error_lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let indented = |error| Self::indented_error(error, width, theme);
        self.error.as_deref().map(indented).unwrap_or_default()
    }

    fn indented_error(error: &str, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let indent = " ".repeat(Self::SIDE_PADDING);
        let wrapped = TextFit::new(error).wrapped(width.saturating_sub(Self::SIDE_PADDING));
        wrapped
            .into_iter()
            .map(|line| Line::styled(format!("{indent}{line}"), theme.error()))
            .collect()
    }

    fn count(&self) -> String {
        format!("{}/{}", self.matches.len(), self.candidates.len())
    }

    fn match_rows(&self, width: usize, rows: usize, theme: &Theme) -> Vec<Line<'static>> {
        let name_room = width.saturating_sub(Self::SIDE_PADDING);
        let indent = " ".repeat(Self::SIDE_PADDING);
        self.matches.lines(rows, theme, |index| {
            let spans = self.candidates[index].spans(name_room, theme);
            once(Span::raw(indent.clone()))
                .chain(spans)
                .collect::<Line>()
        })
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;

    use super::*;
    use crate::domain::SessionState;
    use crate::test_support::ServiceFixture;

    struct FinderFixture;

    impl FinderFixture {
        const WIDTH: usize = 20;
        const HEIGHT: usize = 8;
        const FIRST_MATCH: usize = 2;
        const ALPHA: &'static str = "/repos/alpha";
        const BETA: &'static str = "/repos/beta";

        fn finder() -> SessionFinder {
            let names = ["fix-auth", "dark-mode", "docs"];
            let rows: Vec<SessionSummary> = names
                .iter()
                .map(|name| Self::row(name, Self::ALPHA))
                .collect();
            SessionFinder::open(&rows)
        }

        fn two_repo_finder() -> SessionFinder {
            SessionFinder::open(&[
                Self::row("docs", Self::ALPHA),
                Self::row("fix-auth", Self::BETA),
            ])
        }

        fn row(name: &str, repo: &str) -> SessionSummary {
            ServiceFixture::summary_in(name, repo, SessionState::Waiting)
        }

        fn matches(finder: &SessionFinder) -> Vec<String> {
            Self::rows(finder).split_off(Self::FIRST_MATCH)
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

        fn full_width(text: &str) -> String {
            format!("{text:<width$}", width = Self::WIDTH)
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
                FinderFixture::full_width(" fix-auth"),
                FinderFixture::full_width(" dark-mode"),
                FinderFixture::full_width(" docs"),
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
                FinderFixture::full_width(" fix-auth"),
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
        let row = FinderFixture::row("a-session-name-wider-than-the-finder", FinderFixture::ALPHA);
        let finder = SessionFinder::open(&[row]);

        assert_eq!(FinderFixture::matches(&finder), [" a-session-name-wid…"]);
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
    fn a_failure_shows_between_the_rule_and_the_matches() {
        let mut finder = FinderFixture::finder();

        finder.fail("session fix-auth is stopped".to_string());

        assert_eq!(
            FinderFixture::rows(&finder)[FinderFixture::FIRST_MATCH..][..3],
            [
                " session fix-auth is".to_string(),
                " stopped".to_string(),
                FinderFixture::full_width(" fix-auth"),
            ]
        );
    }

    #[test]
    fn the_next_key_clears_a_failure() {
        let mut finder = FinderFixture::finder();
        finder.fail("session fix-auth is stopped".to_string());

        FinderFixture::press(&mut finder, KeyCode::Down, KeyModifiers::NONE);

        assert_eq!(
            FinderFixture::matches(&finder)[0],
            FinderFixture::full_width(" fix-auth")
        );
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

    #[test]
    fn sessions_in_several_repos_show_their_repo_before_their_name() {
        assert_eq!(
            FinderFixture::matches(&FinderFixture::two_repo_finder()),
            [
                FinderFixture::full_width(" alpha/docs"),
                FinderFixture::full_width(" beta/fix-auth"),
            ]
        );
    }

    #[test]
    fn sessions_in_one_repo_show_bare_names_like_the_flat_session_list() {
        let finder = SessionFinder::open(&[
            FinderFixture::row("docs", FinderFixture::BETA),
            FinderFixture::row("fix-auth", FinderFixture::BETA),
        ]);

        assert_eq!(
            FinderFixture::matches(&finder),
            [
                FinderFixture::full_width(" docs"),
                FinderFixture::full_width(" fix-auth"),
            ]
        );
    }

    #[test]
    fn a_query_matches_the_repo_and_the_name_at_once() {
        let mut finder = SessionFinder::open(&[
            FinderFixture::row("docs", FinderFixture::ALPHA),
            FinderFixture::row("fix-docs", FinderFixture::ALPHA),
            FinderFixture::row("fix-auth", FinderFixture::BETA),
        ]);

        FinderFixture::type_text(&mut finder, "alpha fix");

        assert_eq!(
            FinderFixture::matches(&finder),
            [FinderFixture::full_width(" alpha/fix-docs")]
        );
    }

    #[test]
    fn a_query_ignores_the_repo_when_the_rows_hide_it() {
        let mut finder = FinderFixture::finder();

        FinderFixture::type_text(&mut finder, "alpha");

        assert!(FinderFixture::matches(&finder).is_empty());
    }

    #[test]
    fn enter_attaches_by_the_bare_session_name() {
        let mut finder = FinderFixture::two_repo_finder();

        assert_eq!(
            FinderFixture::enter(&mut finder),
            FinderFixture::attach("docs")
        );
    }
}

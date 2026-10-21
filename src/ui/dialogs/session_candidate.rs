use ratatui::style::Style;
use ratatui::text::Span;

use crate::domain::{RepoGroup, SessionSummary};
use crate::ui::dialogs::fuzzy_search::fuzzy_ranker::FuzzyRanker;
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct SessionCandidate {
    repo_prefix: String,
    name: String,
    matched: Vec<usize>,
}

impl SessionCandidate {
    const REPO_SEPARATOR: char = '/';

    pub fn prefixed(row: &SessionSummary) -> Self {
        let repo_prefix = match RepoGroup::of(row) {
            RepoGroup::Named(repo, _) => format!("{repo}{}", Self::REPO_SEPARATOR),
            RepoGroup::Other => String::new(),
        };
        Self {
            repo_prefix,
            ..Self::bare(row)
        }
    }

    pub fn bare(row: &SessionSummary) -> Self {
        Self {
            repo_prefix: String::new(),
            name: row.name().to_string(),
            matched: Vec::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn shown(&self) -> String {
        format!("{}{}", self.repo_prefix, self.name)
    }

    pub fn mark_matches(&mut self, query: &str, ranker: &mut FuzzyRanker) {
        let positions = ranker.matched_positions(query, &self.shown());
        self.matched = positions.unwrap_or_default();
    }

    pub fn spans(&self, width: usize, theme: &Theme) -> Vec<Span<'static>> {
        let fitted = TextFit::new(&self.shown()).padded(width);
        let styled: Vec<(char, Style)> = fitted
            .chars()
            .enumerate()
            .map(|(position, character)| (character, self.style_at(position, theme)))
            .collect();
        let runs = styled.chunk_by(|first, second| first.1 == second.1);
        runs.map(Self::span).collect()
    }

    fn style_at(&self, position: usize, theme: &Theme) -> Style {
        if self.matched.contains(&position) {
            theme.emphasis()
        } else if position < self.prefix_width() {
            theme.dim()
        } else {
            theme.base()
        }
    }

    fn prefix_width(&self) -> usize {
        self.repo_prefix.chars().count()
    }

    fn span(run: &[(char, Style)]) -> Span<'static> {
        let text: String = run.iter().map(|(character, _)| character).collect();
        Span::styled(text, run[0].1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{SessionState, UnmanagedSession};
    use crate::test_support::ServiceFixture;

    struct CandidateFixture;

    impl CandidateFixture {
        const WIDTH: usize = 20;
        const REPO: &'static str = "/repos/alpha";

        fn prefixed(name: &str) -> SessionCandidate {
            let row = ServiceFixture::summary_in(name, Self::REPO, SessionState::Waiting);
            SessionCandidate::prefixed(&row)
        }

        fn matching(name: &str, query: &str) -> SessionCandidate {
            let mut candidate = Self::prefixed(name);
            candidate.mark_matches(query, &mut FuzzyRanker::for_names());
            candidate
        }

        fn outside_any_repo(label: &str) -> SessionCandidate {
            let stranger = ServiceFixture::session(label);
            let observation = ServiceFixture::waiting(&stranger).without_metadata();
            let row = SessionSummary::unmanaged(UnmanagedSession::new(label, observation));
            SessionCandidate::prefixed(&row)
        }

        fn text_in(candidate: &SessionCandidate, style: Style) -> String {
            let spans = candidate.spans(Self::WIDTH, &Theme);
            let styled = spans.iter().filter(|span| span.style == style);
            styled.map(|span| span.content.as_ref()).collect()
        }
    }

    #[test]
    fn a_session_in_a_repo_shows_its_repo_before_its_name() {
        assert_eq!(CandidateFixture::prefixed("docs").shown(), "alpha/docs");
    }

    #[test]
    fn a_session_outside_any_repo_shows_no_prefix() {
        assert_eq!(
            CandidateFixture::outside_any_repo("scratch").shown(),
            "scratch"
        );
    }

    #[test]
    fn the_repo_prefix_is_dim() {
        let candidate = CandidateFixture::prefixed("docs");

        assert_eq!(CandidateFixture::text_in(&candidate, Theme.dim()), "alpha/");
    }

    #[test]
    fn matched_letters_are_bold() {
        let candidate = CandidateFixture::matching("fix-auth", "fa");

        assert_eq!(
            CandidateFixture::text_in(&candidate, Theme.emphasis()),
            "fa"
        );
    }

    #[test]
    fn a_matched_repo_letter_drops_dim_because_bold_and_faint_cancel_in_some_terminals() {
        let candidate = CandidateFixture::matching("docs", "ph");

        assert_eq!(
            CandidateFixture::text_in(&candidate, Theme.emphasis()),
            "ph"
        );
        assert_eq!(CandidateFixture::text_in(&candidate, Theme.dim()), "ala/");
    }
}

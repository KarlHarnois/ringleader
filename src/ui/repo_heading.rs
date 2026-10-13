use ratatui::text::Span;

use super::repo_group::RepoGroup;
use super::state_indicator::StateIndicator;
use super::theme::Theme;
use crate::domain::{SessionState, SessionSummary};

pub struct RepoHeading<'a> {
    group: RepoGroup,
    number: usize,
    rows: &'a [SessionSummary],
}

impl<'a> RepoHeading<'a> {
    const LAST_NUMBERED: usize = 9;
    const UNNUMBERED: &'static str = " ";
    const UNFOLDED: char = '▾';

    pub fn new(number: usize, rows: &'a [SessionSummary]) -> Self {
        Self {
            group: rows.first().map_or(RepoGroup::Other, RepoGroup::of),
            number,
            rows,
        }
    }

    pub fn spans(&self, tick: u64, theme: &Theme) -> Vec<Span<'static>> {
        let mut spans = self.title_spans(theme);
        spans.extend(self.count_spans(tick, theme));
        spans
    }

    fn title_spans(&self, theme: &Theme) -> Vec<Span<'static>> {
        vec![
            Span::styled(self.number_label(), theme.dim()),
            Span::styled(format!(" {} ", Self::UNFOLDED), theme.base()),
            Span::styled(self.group.to_string(), theme.emphasis()),
            Span::styled(" ", theme.base()),
            Span::styled(format!("({})", self.rows.len()), theme.dim()),
            Span::styled(" ", theme.base()),
        ]
    }

    fn count_spans(&self, tick: u64, theme: &Theme) -> Vec<Span<'static>> {
        SessionState::ALL
            .into_iter()
            .filter_map(|state| self.count_span(state, tick, theme))
            .flat_map(|count| [Span::styled(" ", theme.base()), count])
            .collect()
    }

    fn count_span(&self, state: SessionState, tick: u64, theme: &Theme) -> Option<Span<'static>> {
        let count = self.rows.iter().filter(|row| row.state() == state).count();
        let indicator = StateIndicator::new(state, tick);
        let text = format!("{}{count}", indicator.glyph());
        (count > 0).then(|| Span::styled(text, indicator.style(theme)))
    }

    fn number_label(&self) -> String {
        if self.number > Self::LAST_NUMBERED {
            return Self::UNNUMBERED.to_string();
        }
        self.number.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ServiceFixture;

    struct HeadingFixture;

    impl HeadingFixture {
        fn row(name: &str, state: SessionState) -> SessionSummary {
            let session =
                ServiceFixture::session_at(name, "/repos/alpha", ServiceFixture::CREATED_AT);
            SessionSummary::managed(session, state, false)
        }

        fn text(heading: &RepoHeading) -> String {
            let spans = heading.spans(0, &Theme);
            spans.iter().map(|span| span.content.as_ref()).collect()
        }
    }

    #[test]
    fn the_counts_follow_the_session_state_order() {
        let rows = [
            HeadingFixture::row("docs", SessionState::Stopped),
            HeadingFixture::row("fix-auth", SessionState::Working),
            HeadingFixture::row("dark-mode", SessionState::Waiting),
            HeadingFixture::row("spike-tui", SessionState::Waiting),
        ];

        let heading = RepoHeading::new(1, &rows);

        assert_eq!(HeadingFixture::text(&heading), "1 ▾ alpha (4)  ⠋1 ●2 ○1");
    }

    #[test]
    fn headings_after_the_ninth_show_a_blank_for_their_number() {
        let rows = [HeadingFixture::row("docs", SessionState::Waiting)];

        let heading = RepoHeading::new(10, &rows);

        assert_eq!(HeadingFixture::text(&heading), "  ▾ alpha (1)  ●1");
    }
}

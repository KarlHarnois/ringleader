use std::collections::HashSet;
use std::iter::once;

use ratatui::text::Span;

use super::list_selection::ListSelection;
use super::state_indicator::StateIndicator;
use crate::domain::{RepoGroup, SessionFilter, SessionState, SessionSummary};
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct RepoHeading<'a> {
    group: RepoGroup,
    number: usize,
    folded: bool,
    rows: &'a [SessionSummary],
    filter: SessionFilter,
}

impl<'a> RepoHeading<'a> {
    pub const LEAD_WIDTH: usize = 4;
    const LAST_NUMBERED: usize = 9;
    const UNNUMBERED: &'static str = " ";
    const UNFOLDED: char = '▾';
    const FOLDED: char = '▸';

    pub fn new(number: usize, rows: &'a [SessionSummary], filter: SessionFilter) -> Self {
        let group = rows.first().map_or(RepoGroup::Other, RepoGroup::of);
        Self {
            group,
            number,
            folded: false,
            rows,
            filter,
        }
    }

    pub fn visible(
        number: usize,
        rows: &'a [SessionSummary],
        filter: SessionFilter,
    ) -> Option<Self> {
        let heading = Self::new(number, rows, filter);
        heading.has_visible_rows().then_some(heading)
    }

    pub fn folded_among(self, folded: &HashSet<RepoGroup>) -> Self {
        let folded = folded.contains(&self.group);
        Self { folded, ..self }
    }

    pub fn group(&self) -> &RepoGroup {
        &self.group
    }

    pub fn number(&self) -> usize {
        self.number
    }

    pub fn jump_target(&self) -> ListSelection {
        match self.shown_rows().next() {
            Some(row) => ListSelection::Session(row.name().to_string()),
            None => ListSelection::Repo(self.group.clone()),
        }
    }

    pub fn shown_rows(&self) -> impl Iterator<Item = &'a SessionSummary> + '_ {
        let shown = !self.folded;
        self.filtered_rows().filter(move |_| shown)
    }

    pub fn has_visible_rows(&self) -> bool {
        self.filtered_rows().next().is_some()
    }

    fn filtered_rows(&self) -> impl Iterator<Item = &'a SessionSummary> + '_ {
        let filter = self.filter;
        self.rows.iter().filter(move |row| filter.allows(row))
    }

    pub fn spans(&self, tick: u64, theme: &Theme, width: usize) -> Vec<Span<'static>> {
        let lead = self.lead_spans(theme);
        let tail = self.tail_spans(tick, theme);
        let room_for_name = width.saturating_sub(Self::width_of(&lead) + Self::width_of(&tail));
        let name = self.name_span(theme, room_for_name);
        lead.into_iter().chain(once(name)).chain(tail).collect()
    }

    fn lead_spans(&self, theme: &Theme) -> Vec<Span<'static>> {
        vec![
            Span::styled(self.number_label(), theme.dim()),
            Span::styled(format!(" {} ", self.arrow()), theme.base()),
        ]
    }

    fn name_span(&self, theme: &Theme, room: usize) -> Span<'static> {
        let name = TextFit::new(&self.group.to_string()).right_truncated(room);
        Span::styled(name, theme.emphasis())
    }

    fn tail_spans(&self, tick: u64, theme: &Theme) -> Vec<Span<'static>> {
        if !self.folded {
            return Vec::new();
        }
        self.count_spans(tick, theme)
    }

    fn count_spans(&self, tick: u64, theme: &Theme) -> Vec<Span<'static>> {
        SessionState::ALL
            .into_iter()
            .filter_map(|state| self.count_span(state, tick, theme))
            .flat_map(|count| [Span::styled(" ", theme.base()), count])
            .collect()
    }

    fn count_span(&self, state: SessionState, tick: u64, theme: &Theme) -> Option<Span<'static>> {
        let count = self
            .filtered_rows()
            .filter(|row| row.state() == state)
            .count();
        let indicator = StateIndicator::new(state, tick);
        let text = format!("{}{count}", indicator.glyph());
        (count > 0).then(|| Span::styled(text, indicator.style(theme)))
    }

    fn width_of(spans: &[Span]) -> usize {
        spans.iter().map(Span::width).sum()
    }

    fn arrow(&self) -> char {
        if self.folded {
            return Self::FOLDED;
        }
        Self::UNFOLDED
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
        const WIDE: usize = 80;

        fn row(name: &str, state: SessionState) -> SessionSummary {
            ServiceFixture::summary_in(name, "/repos/alpha", state)
        }

        fn favorite(name: &str, state: SessionState) -> SessionSummary {
            let session =
                ServiceFixture::session_at(name, "/repos/alpha", ServiceFixture::CREATED_AT)
                    .with_favorite(true);
            SessionSummary::managed(session, state, false)
        }

        fn folded(rows: &[SessionSummary]) -> RepoHeading<'_> {
            let folded = HashSet::from([RepoGroup::of(&rows[0])]);
            RepoHeading::new(1, rows, SessionFilter::All).folded_among(&folded)
        }

        fn text(heading: &RepoHeading) -> String {
            Self::text_within(heading, Self::WIDE)
        }

        fn text_within(heading: &RepoHeading, width: usize) -> String {
            let spans = heading.spans(0, &Theme, width);
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

        let heading = HeadingFixture::folded(&rows);

        assert_eq!(HeadingFixture::text(&heading), "1 ▸ alpha ⠋1 ●2 ○1");
    }

    #[test]
    fn a_folded_heading_shows_the_folded_arrow_and_no_rows() {
        let rows = [HeadingFixture::row("docs", SessionState::Waiting)];

        let heading = HeadingFixture::folded(&rows);

        assert_eq!(HeadingFixture::text(&heading), "1 ▸ alpha ●1");
        assert!(heading.shown_rows().next().is_none());
    }

    #[test]
    fn the_counts_follow_the_favorites_filter() {
        let rows = [
            HeadingFixture::favorite("docs", SessionState::Waiting),
            HeadingFixture::row("fix-auth", SessionState::Working),
        ];
        let folded = HashSet::from([RepoGroup::of(&rows[0])]);
        let heading = RepoHeading::new(1, &rows, SessionFilter::Favorites).folded_among(&folded);

        assert_eq!(HeadingFixture::text(&heading), "1 ▸ alpha ●1");
    }

    #[test]
    fn an_expanded_heading_shows_no_counts() {
        let rows = [HeadingFixture::row("docs", SessionState::Waiting)];

        let heading = RepoHeading::new(1, &rows, SessionFilter::All);

        assert_eq!(HeadingFixture::text(&heading), "1 ▾ alpha");
    }

    #[test]
    fn a_narrow_heading_shortens_the_repo_name_and_keeps_the_counts_whole() {
        let repo = "/repos/example-project-dashboard";
        let rows = [
            ServiceFixture::summary_in("docs", repo, SessionState::Working),
            ServiceFixture::summary_in("fix-auth", repo, SessionState::Waiting),
        ];

        let heading = HeadingFixture::folded(&rows);

        assert_eq!(
            HeadingFixture::text_within(&heading, 26),
            "1 ▸ example-project… ⠋1 ●1"
        );
    }

    #[test]
    fn headings_after_the_ninth_show_a_blank_for_their_number() {
        let rows = [HeadingFixture::row("docs", SessionState::Waiting)];

        let heading = RepoHeading::new(10, &rows, SessionFilter::All);

        assert_eq!(HeadingFixture::text(&heading), "  ▾ alpha");
    }

    #[test]
    fn every_repo_name_starts_after_the_lead_width() {
        let rows = [HeadingFixture::row("docs", SessionState::Waiting)];

        for number in [1, 10] {
            let text = HeadingFixture::text(&RepoHeading::new(number, &rows, SessionFilter::All));
            let name: String = text.chars().skip(RepoHeading::LEAD_WIDTH).collect();
            assert_eq!(name, "alpha");
        }
    }
}

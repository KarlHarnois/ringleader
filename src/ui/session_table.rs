use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::empty_state::EmptyState;
use super::home_view::LayoutTier;
use super::table_columns::TableColumns;
use super::theme::Theme;
use crate::domain::{DiffStat, SessionState, SessionSummary};

pub struct SessionTable<'a> {
    rows: &'a [SessionSummary],
    selection: Option<&'a str>,
    tick: u64,
    theme: Theme,
    tier: LayoutTier,
}

impl<'a> SessionTable<'a> {
    const NO_DIFF: &'static str = "—";

    pub fn new(
        rows: &'a [SessionSummary],
        selection: Option<&'a str>,
        tick: u64,
        theme: Theme,
    ) -> Self {
        Self {
            rows,
            selection,
            tick,
            theme,
            tier: LayoutTier::Full,
        }
    }

    pub fn for_tier(self, tier: LayoutTier) -> Self {
        Self { tier, ..self }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default().borders(Borders::ALL).title(" Sessions ");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let columns = self.columns(usize::from(inner.width));
        let mut lines = Vec::new();
        if self.shows_header() {
            lines.push(Line::styled(columns.header(), self.theme.emphasis()));
        }
        if !self.has_rows() {
            Self::render_empty(lines, frame, inner);
            return;
        }
        let room = usize::from(inner.height).saturating_sub(lines.len());
        lines.extend(
            self.visible_rows(room)
                .map(|row| self.row_line(row, &columns)),
        );
        frame.render_widget(Paragraph::new(lines), inner);
    }

    pub fn has_rows(&self) -> bool {
        !self.rows.is_empty()
    }

    fn render_empty(header: Vec<Line>, frame: &mut Frame, inner: Rect) {
        let header_height = u16::try_from(header.len()).unwrap_or(u16::MAX);
        let [header_area, body] =
            Layout::vertical([Constraint::Length(header_height), Constraint::Fill(1)]).areas(inner);
        frame.render_widget(Paragraph::new(header), header_area);
        EmptyState.render(frame, body);
    }

    fn shows_header(&self) -> bool {
        self.tier != LayoutTier::Minimal
    }

    fn visible_rows(&self, room: usize) -> impl Iterator<Item = &'a SessionSummary> {
        let selected = self
            .selection
            .and_then(|name| self.rows.iter().position(|row| row.name() == name))
            .unwrap_or(0);
        self.rows
            .iter()
            .skip(Self::first_visible(selected, room))
            .take(room)
    }

    pub fn first_visible(selected: usize, room: usize) -> usize {
        (selected + 1).saturating_sub(room)
    }

    fn columns(&self, inner_width: usize) -> TableColumns {
        let names = self.rows.iter().map(|row| row.name().to_string());
        let mut columns = TableColumns::new(inner_width, names);
        if self.shows_header() {
            columns = columns
                .with_branch(self.rows.iter().map(Self::branch_text))
                .with_diff(self.rows.iter().map(Self::diff_text));
        }
        columns.fitted()
    }

    fn row_line(&self, row: &SessionSummary, columns: &TableColumns) -> Line<'static> {
        let indicator = StateIndicator::new(row.state(), self.tick);
        let diff_text = Self::diff_text(row);
        let text = columns.row(row.name(), &Self::branch_text(row), &diff_text);
        if self.selection == Some(row.name()) {
            let selected = self.theme.selected();
            return Line::from(vec![
                Span::styled(indicator.glyph().to_string(), selected),
                Span::styled(text, selected),
            ]);
        }
        let mut spans = vec![Span::styled(
            indicator.glyph().to_string(),
            indicator.style(&self.theme),
        )];
        spans.extend(self.colored_diff(text, &diff_text, row.diff()));
        Line::from(spans)
    }

    fn colored_diff(
        &self,
        text: String,
        diff_text: &str,
        diff: Option<DiffStat>,
    ) -> Vec<Span<'static>> {
        let (Some(diff), Some(start)) = (
            diff,
            text.rfind(diff_text).filter(|_| !diff_text.is_empty()),
        ) else {
            return vec![Span::styled(text, self.theme.base())];
        };
        let (before, rest) = text.split_at(start);
        let after = &rest[diff_text.len()..];
        let untracked = diff.untracked_marker().map(|marker| format!(" {marker}"));
        vec![
            Span::styled(before.to_string(), self.theme.base()),
            Span::styled(format!("+{}", diff.insertions()), self.theme.insertions()),
            Span::styled(format!(" -{}", diff.deletions()), self.theme.deletions()),
            Span::styled(
                format!("{}{after}", untracked.unwrap_or_default()),
                self.theme.base(),
            ),
        ]
    }

    fn branch_text(row: &SessionSummary) -> String {
        row.metadata()
            .map(|metadata| metadata.branch().to_string())
            .unwrap_or_default()
    }

    fn diff_text(row: &SessionSummary) -> String {
        if row.session().is_none() {
            return Self::NO_DIFF.to_string();
        }
        row.diff().map_or(String::new(), |diff| diff.to_string())
    }
}

pub struct StateIndicator {
    state: SessionState,
    tick: u64,
}

impl StateIndicator {
    const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

    pub fn new(state: SessionState, tick: u64) -> Self {
        Self { state, tick }
    }

    pub fn glyph(&self) -> char {
        match self.state {
            SessionState::Working => {
                let frame = self.tick % Self::SPINNER.len() as u64;
                Self::SPINNER[usize::try_from(frame).unwrap_or(0)]
            }
            SessionState::Waiting => '●',
            SessionState::Exited => '✗',
            SessionState::Stopped => '○',
            SessionState::Unmanaged => '?',
        }
    }

    pub fn style(&self, theme: &Theme) -> Style {
        theme.state(self.state)
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::{Color, Modifier};

    use super::*;
    use crate::domain::{DiffStat, UnmanagedSession};
    use crate::test_support::ServiceFixture;

    struct TableFixture;

    impl TableFixture {
        fn managed(name: &str, state: SessionState) -> SessionSummary {
            SessionSummary::managed(ServiceFixture::session(name), state, false)
        }

        fn unmanaged() -> SessionSummary {
            let stranger = ServiceFixture::session("mystery");
            SessionSummary::unmanaged(UnmanagedSession::new(
                "mystery",
                ServiceFixture::waiting(&stranger).without_metadata(),
            ))
        }

        fn render(
            rows: &[SessionSummary],
            selection: Option<&str>,
            tick: u64,
            height: u16,
        ) -> Terminal<TestBackend> {
            let mut terminal = Terminal::new(TestBackend::new(50, height)).unwrap();
            terminal
                .draw(|frame| {
                    SessionTable::new(rows, selection, tick, Theme).render(frame, frame.area())
                })
                .unwrap();
            terminal
        }

        fn indicator_cell(rows: &[SessionSummary], tick: u64) -> (String, Color) {
            let terminal = Self::render(rows, None, tick, 6);
            let cell = &terminal.backend().buffer()[(1, 2)];
            (cell.symbol().to_string(), cell.fg)
        }

        fn line(terminal: &Terminal<TestBackend>, row: u16) -> String {
            let buffer = terminal.backend().buffer();
            (0..buffer.area.width)
                .map(|column| buffer[(column, row)].symbol())
                .collect()
        }
    }

    #[test]
    fn a_working_row_shows_the_spinner_frame_of_the_tick() {
        let rows = [TableFixture::managed("fix-auth", SessionState::Working)];

        assert_eq!(TableFixture::indicator_cell(&rows, 2).0, "⠹");
    }

    #[test]
    fn the_spinner_advances_with_the_tick() {
        let rows = [TableFixture::managed("fix-auth", SessionState::Working)];

        let first = TableFixture::indicator_cell(&rows, 0).0;
        let second = TableFixture::indicator_cell(&rows, 1).0;

        assert_ne!(first, second);
    }

    #[test]
    fn a_waiting_row_shows_a_green_bullet() {
        let rows = [TableFixture::managed("fix-auth", SessionState::Waiting)];

        assert_eq!(
            TableFixture::indicator_cell(&rows, 0),
            ("●".to_string(), Color::Green)
        );
    }

    #[test]
    fn an_exited_row_shows_a_red_cross() {
        let rows = [TableFixture::managed("fix-auth", SessionState::Exited)];

        assert_eq!(
            TableFixture::indicator_cell(&rows, 0),
            ("✗".to_string(), Color::Red)
        );
    }

    #[test]
    fn a_stopped_row_shows_a_hollow_circle() {
        let rows = [TableFixture::managed("fix-auth", SessionState::Stopped)];

        assert_eq!(TableFixture::indicator_cell(&rows, 0).0, "○");
    }

    #[test]
    fn an_unmanaged_row_shows_a_yellow_question_mark() {
        assert_eq!(
            TableFixture::indicator_cell(&[TableFixture::unmanaged()], 0),
            ("?".to_string(), Color::Yellow)
        );
    }

    #[test]
    fn an_unmanaged_row_shows_a_dash_for_its_diff() {
        let terminal = TableFixture::render(&[TableFixture::unmanaged()], None, 0, 6);

        assert!(TableFixture::line(&terminal, 2).contains("mystery"));
        assert!(TableFixture::line(&terminal, 2).contains(" —"));
    }

    #[test]
    fn a_managed_row_shows_its_diff_stat() {
        let row = TableFixture::managed("fix-auth", SessionState::Waiting)
            .with_diff(DiffStat::from_changes([], 2));

        let terminal = TableFixture::render(&[row], None, 0, 6);

        assert!(TableFixture::line(&terminal, 2).contains("+0 -0 ?2"));
    }

    #[test]
    fn insertions_show_in_green_and_deletions_in_red() {
        let row = TableFixture::managed("fix-auth", SessionState::Waiting).with_diff(
            DiffStat::from_changes(
                [crate::domain::FileChange::Text {
                    insertions: 12,
                    deletions: 4,
                }],
                0,
            ),
        );

        let terminal = TableFixture::render(&[row], None, 0, 6);
        let line = TableFixture::line(&terminal, 2);
        let column = |text: &str| {
            let byte = line.find(text).unwrap();
            u16::try_from(line[..byte].chars().count()).unwrap()
        };
        let (plus, minus) = (column("+12"), column("-4"));
        let buffer = terminal.backend().buffer();

        assert!(line.contains("+12 -4 "));
        assert!(!line.contains("?0"));
        assert_eq!(buffer[(plus, 2)].fg, Color::Green);
        assert_eq!(buffer[(minus, 2)].fg, Color::Red);
    }

    #[test]
    fn only_the_selected_row_is_reversed_across_the_whole_width() {
        let rows = [
            TableFixture::managed("dark-mode", SessionState::Waiting),
            TableFixture::managed("fix-auth", SessionState::Waiting),
        ];

        let terminal = TableFixture::render(&rows, Some("fix-auth"), 0, 6);
        let buffer = terminal.backend().buffer();

        assert!(!buffer[(1, 2)].modifier.contains(Modifier::REVERSED));
        assert!(buffer[(1, 3)].modifier.contains(Modifier::REVERSED));
        assert!(buffer[(48, 3)].modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn a_selection_below_the_fold_scrolls_into_view() {
        let rows: Vec<SessionSummary> = ["a-one", "b-two", "c-three", "d-four"]
            .iter()
            .map(|name| TableFixture::managed(name, SessionState::Waiting))
            .collect();

        let terminal = TableFixture::render(&rows, Some("d-four"), 0, 5);

        assert!(TableFixture::line(&terminal, 3).contains("d-four"));
        assert!(TableFixture::line(&terminal, 2).contains("c-three"));
    }

    #[test]
    fn the_minimal_tier_has_no_header() {
        let rows = [TableFixture::managed("fix-auth", SessionState::Waiting)];
        let mut terminal = Terminal::new(TestBackend::new(36, 5)).unwrap();

        terminal
            .draw(|frame| {
                SessionTable::new(&rows, None, 0, Theme)
                    .for_tier(LayoutTier::Minimal)
                    .render(frame, frame.area())
            })
            .unwrap();

        assert!(TableFixture::line(&terminal, 1).starts_with("│● fix-auth"));
    }
}

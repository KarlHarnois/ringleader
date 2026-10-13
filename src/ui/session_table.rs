use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::colored_row::ColoredRow;
use super::empty_state::EmptyState;
use super::home_view::LayoutTier;
use super::list_selection::ListSelection;
use super::session_tree::{SessionTree, TreeBranch, TreeLine};
use super::state_indicator::StateIndicator;
use super::table_columns::TableColumns;
use super::theme::Theme;
use crate::domain::{DiffStat, SessionSummary};

pub struct SessionTable<'a> {
    tree: SessionTree<'a>,
    selection: Option<&'a ListSelection>,
    tick: u64,
    theme: Theme,
    tier: LayoutTier,
}

impl<'a> SessionTable<'a> {
    const NO_DIFF: &'static str = "—";

    pub fn new(
        tree: SessionTree<'a>,
        selection: Option<&'a ListSelection>,
        tick: u64,
        theme: Theme,
    ) -> Self {
        Self {
            tree,
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
        let width = usize::from(inner.width);
        let columns = self.columns(width);
        let mut lines = Vec::new();
        if self.shows_header() {
            lines.push(Line::styled(self.header(&columns), self.theme.emphasis()));
        }
        if !self.has_rows() {
            Self::render_empty(lines, frame, inner);
            return;
        }
        let room = usize::from(inner.height).saturating_sub(lines.len());
        let visible = self.tree.visible(self.selection, room);
        lines.extend(visible.map(|line| self.line(line, &columns, width)));
        frame.render_widget(Paragraph::new(lines), inner);
    }

    pub fn has_rows(&self) -> bool {
        !self.tree.rows().is_empty()
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

    fn header(&self, columns: &TableColumns) -> String {
        format!("{}{}", " ".repeat(self.branch_width()), columns.header())
    }

    fn branch_width(&self) -> usize {
        if self.tree.is_grouped() {
            return TreeBranch::WIDTH;
        }
        0
    }

    fn columns(&self, inner_width: usize) -> TableColumns {
        let rows = self.tree.rows();
        let names = rows.iter().map(|row| row.name().to_string());
        let mut columns = TableColumns::new(inner_width.saturating_sub(self.branch_width()), names);
        if self.shows_header() {
            columns = columns.with_diff(rows.iter().map(Self::diff_text));
            if self.has_pull_requests() {
                columns = columns.with_pull_request(rows.iter().map(Self::pull_request_text));
            }
        }
        columns.fitted()
    }

    fn has_pull_requests(&self) -> bool {
        self.tree
            .rows()
            .iter()
            .any(|row| row.pull_request().is_some())
    }

    fn line(&self, line: &TreeLine, columns: &TableColumns, width: usize) -> Line<'static> {
        let spans = match line {
            TreeLine::Heading(heading) => heading.spans(self.tick, &self.theme),
            TreeLine::Session { row, branch } => self.row_spans(row, *branch, columns),
        };
        if self.is_selected(line) {
            return self.reversed(&spans, width);
        }
        Line::from(spans)
    }

    fn is_selected(&self, line: &TreeLine) -> bool {
        self.selection == Some(&line.selection())
    }

    fn reversed(&self, spans: &[Span], width: usize) -> Line<'static> {
        let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
        Line::from(Span::styled(
            format!("{text:<width$}"),
            self.theme.selected(),
        ))
    }

    fn row_spans(
        &self,
        row: &SessionSummary,
        branch: TreeBranch,
        columns: &TableColumns,
    ) -> Vec<Span<'static>> {
        let indicator = StateIndicator::new(row.state(), self.tick);
        let pull_request_text = Self::pull_request_text(row);
        let diff_text = Self::diff_text(row);
        let text = columns.row(row.name(), &pull_request_text, &diff_text);
        let mut spans = vec![
            Span::styled(branch.prefix(), self.theme.dim()),
            Span::styled(indicator.glyph().to_string(), indicator.style(&self.theme)),
        ];
        spans.extend(self.colored_cells(&text, row, &pull_request_text, &diff_text));
        spans
    }

    fn colored_cells(
        &self,
        text: &str,
        row: &SessionSummary,
        pull_request_text: &str,
        diff_text: &str,
    ) -> Vec<Span<'static>> {
        let mut colored = ColoredRow::new(text, self.theme.base());
        if let Some(pull_request) = row.pull_request() {
            let style = self.theme.pull_request(pull_request.state());
            let cell = vec![Span::styled(pull_request_text.to_string(), style)];
            colored = colored.color_first(pull_request_text, cell);
        }
        if let Some(diff) = row.diff() {
            colored = colored.color_last(diff_text, self.diff_spans(diff));
        }
        colored.into_spans()
    }

    fn diff_spans(&self, diff: DiffStat) -> Vec<Span<'static>> {
        let mut spans = vec![
            Span::styled(format!("+{}", diff.insertions()), self.theme.insertions()),
            Span::styled(format!(" -{}", diff.deletions()), self.theme.deletions()),
        ];
        spans.extend(
            diff.untracked_marker()
                .map(|marker| Span::styled(format!(" {marker}"), self.theme.base())),
        );
        spans
    }

    fn pull_request_text(row: &SessionSummary) -> String {
        row.pull_request()
            .map_or(String::new(), |pull_request| pull_request.to_string())
    }

    fn diff_text(row: &SessionSummary) -> String {
        if row.session().is_none() {
            return Self::NO_DIFF.to_string();
        }
        row.diff().map_or(String::new(), |diff| diff.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::{Color, Modifier};

    use super::*;
    use crate::domain::{DiffStat, PullRequestState, SessionState, UnmanagedSession};
    use crate::test_support::ServiceFixture;
    use crate::ui::repo_group::RepoGroup;

    struct TableFixture;

    impl TableFixture {
        fn managed(name: &str, state: SessionState) -> SessionSummary {
            SessionSummary::managed(ServiceFixture::session(name), state, false)
        }

        fn in_repo(name: &str, repo: &str, state: SessionState) -> SessionSummary {
            let session = ServiceFixture::session_at(name, repo, ServiceFixture::CREATED_AT);
            SessionSummary::managed(session, state, false)
        }

        fn two_repos() -> Vec<SessionSummary> {
            vec![
                Self::in_repo("fix-auth", "/repos/alpha", SessionState::Working),
                Self::in_repo("dark-mode", "/repos/alpha", SessionState::Waiting),
                Self::in_repo("docs", "/repos/beta", SessionState::Exited),
            ]
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
            let selection = selection.map(|name| ListSelection::Session(name.to_string()));
            Self::render_selecting(rows, selection.as_ref(), tick, height)
        }

        fn render_selecting(
            rows: &[SessionSummary],
            selection: Option<&ListSelection>,
            tick: u64,
            height: u16,
        ) -> Terminal<TestBackend> {
            let mut terminal = Terminal::new(TestBackend::new(50, height)).unwrap();
            let tree = || SessionTree::new(rows, &HashSet::new());
            terminal
                .draw(|frame| {
                    SessionTable::new(tree(), selection, tick, Theme).render(frame, frame.area())
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
    fn a_row_with_a_pull_request_shows_its_number_in_the_state_colour() {
        let row = TableFixture::managed("fix-auth", SessionState::Waiting)
            .with_pull_request(ServiceFixture::pull_request(142, PullRequestState::Open));

        let terminal = TableFixture::render(&[row], None, 0, 6);
        let line = TableFixture::line(&terminal, 2);
        let byte = line.find("#142").unwrap();
        let column = u16::try_from(line[..byte].chars().count()).unwrap();
        let buffer = terminal.backend().buffer();

        assert!(line.contains("#142"));
        assert_eq!(buffer[(column, 2)].fg, Color::Green);
    }

    #[test]
    fn a_merged_pull_request_shows_in_magenta() {
        let row = TableFixture::managed("fix-auth", SessionState::Waiting)
            .with_pull_request(ServiceFixture::pull_request(9, PullRequestState::Merged));

        let terminal = TableFixture::render(&[row], None, 0, 6);
        let line = TableFixture::line(&terminal, 2);
        let byte = line.find("#9").unwrap();
        let column = u16::try_from(line[..byte].chars().count()).unwrap();
        let buffer = terminal.backend().buffer();

        assert_eq!(buffer[(column, 2)].fg, Color::Magenta);
    }

    #[test]
    fn a_draft_pull_request_shows_in_yellow() {
        let row = TableFixture::managed("fix-auth", SessionState::Waiting)
            .with_pull_request(ServiceFixture::pull_request(5, PullRequestState::Draft));

        let terminal = TableFixture::render(&[row], None, 0, 6);
        let line = TableFixture::line(&terminal, 2);
        let byte = line.find("#5").unwrap();
        let column = u16::try_from(line[..byte].chars().count()).unwrap();
        let buffer = terminal.backend().buffer();

        assert_eq!(buffer[(column, 2)].fg, Color::Yellow);
    }

    #[test]
    fn a_closed_pull_request_shows_in_red() {
        let row = TableFixture::managed("fix-auth", SessionState::Waiting)
            .with_pull_request(ServiceFixture::pull_request(3, PullRequestState::Closed));

        let terminal = TableFixture::render(&[row], None, 0, 6);
        let line = TableFixture::line(&terminal, 2);
        let byte = line.find("#3").unwrap();
        let column = u16::try_from(line[..byte].chars().count()).unwrap();
        let buffer = terminal.backend().buffer();

        assert_eq!(buffer[(column, 2)].fg, Color::Red);
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
    fn a_heading_shows_its_number_arrow_count_and_state_counts() {
        let terminal = TableFixture::render(&TableFixture::two_repos(), None, 0, 8);

        assert!(TableFixture::line(&terminal, 2).starts_with("│1 ▾ alpha (2)  ⠋1 ●1 "));
        assert!(TableFixture::line(&terminal, 5).starts_with("│2 ▾ beta (1)  ✗1 "));
    }

    #[test]
    fn a_heading_dims_its_number_and_count_and_bolds_its_repo() {
        let terminal = TableFixture::render(&TableFixture::two_repos(), None, 0, 8);
        let buffer = terminal.backend().buffer();

        assert!(buffer[(1, 2)].modifier.contains(Modifier::DIM));
        assert!(buffer[(5, 2)].modifier.contains(Modifier::BOLD));
        assert!(buffer[(11, 2)].modifier.contains(Modifier::DIM));
        assert_eq!(buffer[(19, 2)].fg, Color::Green);
    }

    #[test]
    fn sessions_hang_off_their_heading_on_dim_branches() {
        let terminal = TableFixture::render(&TableFixture::two_repos(), None, 0, 8);
        let buffer = terminal.backend().buffer();

        assert!(TableFixture::line(&terminal, 3).starts_with("│  ├─ ⠋ fix-auth"));
        assert!(TableFixture::line(&terminal, 4).starts_with("│  └─ ● dark-mode"));
        assert!(buffer[(3, 3)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn a_selected_heading_is_reversed_across_the_whole_width() {
        let rows = TableFixture::two_repos();
        let alpha = ListSelection::Repo(RepoGroup::of(&rows[0]));

        let terminal = TableFixture::render_selecting(&rows, Some(&alpha), 0, 8);
        let buffer = terminal.backend().buffer();

        assert!(buffer[(1, 2)].modifier.contains(Modifier::REVERSED));
        assert!(buffer[(48, 2)].modifier.contains(Modifier::REVERSED));
        assert!(!buffer[(1, 3)].modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn the_column_header_is_indented_past_the_branches() {
        let terminal = TableFixture::render(&TableFixture::two_repos(), None, 0, 8);

        assert!(TableFixture::line(&terminal, 1).starts_with("│       Name "));
    }

    #[test]
    fn the_minimal_tier_has_no_header() {
        let rows = [TableFixture::managed("fix-auth", SessionState::Waiting)];
        let mut terminal = Terminal::new(TestBackend::new(36, 5)).unwrap();

        terminal
            .draw(|frame| {
                SessionTable::new(SessionTree::new(&rows, &HashSet::new()), None, 0, Theme)
                    .for_tier(LayoutTier::Minimal)
                    .render(frame, frame.area())
            })
            .unwrap();

        assert!(TableFixture::line(&terminal, 1).starts_with("│● fix-auth"));
    }

    #[test]
    fn the_empty_state_never_draws_over_the_header() {
        let mut terminal = Terminal::new(TestBackend::new(64, 10)).unwrap();

        terminal
            .draw(|frame| {
                SessionTable::new(SessionTree::new(&[], &HashSet::new()), None, 0, Theme)
                    .render(frame, frame.area())
            })
            .unwrap();

        assert!(TableFixture::line(&terminal, 1).starts_with("│  Name"));
    }
}

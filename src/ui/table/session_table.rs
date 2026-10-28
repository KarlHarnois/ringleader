use std::collections::HashSet;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::colored_row::ColoredRow;
use super::list_selection::ListSelection;
use super::repo_heading::RepoHeading;
use super::session_tree::{SessionTree, TreeBranch, TreeLine};
use super::state_indicator::StateIndicator;
use super::table_columns::{RowCells, TableColumns};
use super::viewport::Viewport;
use crate::domain::{DiffStat, PullRequest, PullRequestState, SessionMetadata, SessionSummary};
use crate::ui::home::empty_state::EmptyState;
use crate::ui::home::home_view::LayoutTier;
use crate::ui::theme::Theme;

pub struct SessionTable<'a> {
    tree: SessionTree<'a>,
    selection: Option<&'a ListSelection>,
    marked: Vec<ListSelection>,
    viewport: Viewport,
    tick: u64,
    theme: Theme,
    tier: LayoutTier,
    out_of_focus: bool,
}

impl<'a> SessionTable<'a> {
    pub const APPROVED_CHECK: char = '✓';
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
            marked: Vec::new(),
            viewport: Viewport::default(),
            tick,
            theme,
            tier: LayoutTier::Full,
            out_of_focus: false,
        }
    }

    pub fn with_viewport(self, viewport: Viewport) -> Self {
        Self { viewport, ..self }
    }

    pub fn with_marked(self, marked: Vec<ListSelection>) -> Self {
        Self { marked, ..self }
    }

    pub fn for_tier(self, tier: LayoutTier) -> Self {
        Self { tier, ..self }
    }

    pub fn out_of_focus(self) -> Self {
        Self {
            out_of_focus: true,
            ..self
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(self.theme.border())
            .title(" Sessions ");
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
        let visible = self
            .tree
            .visible(self.selection, self.viewport.with_room(room));
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
        columns.header(self.branch_width(), self.name_title_column())
    }

    fn branch_width(&self) -> usize {
        if self.tree.is_grouped() {
            return TreeBranch::WIDTH;
        }
        0
    }

    fn name_title_column(&self) -> usize {
        if self.tree.is_grouped() {
            return RepoHeading::LEAD_WIDTH;
        }
        TableColumns::NAME_COLUMN
    }

    fn columns(&self, inner_width: usize) -> TableColumns {
        let rows = self.tree.rows();
        let names = rows.iter().map(|row| row.name().to_string());
        let columns = TableColumns::new(inner_width.saturating_sub(self.branch_width()), names);
        if !self.shows_header() {
            return columns.fitted();
        }
        self.with_optional_columns(columns).fitted()
    }

    fn with_optional_columns(&self, columns: TableColumns) -> TableColumns {
        let rows = self.tree.rows();
        let mut columns = columns.with_diff(rows.iter().map(Self::diff_text));
        if self.has_pull_requests() {
            columns = columns.with_pull_request(rows.iter().map(Self::pull_request_text));
        }
        if self.has_several_agents() {
            columns = columns.with_agent(rows.iter().map(Self::agent_text));
        }
        columns
    }

    fn has_pull_requests(&self) -> bool {
        self.tree
            .rows()
            .iter()
            .any(|row| row.pull_request().is_some())
    }

    fn has_several_agents(&self) -> bool {
        let metadata = self.tree.rows().iter().filter_map(SessionSummary::metadata);
        let agents: HashSet<&str> = metadata.map(SessionMetadata::agent).collect();
        agents.len() > 1
    }

    fn line(&self, line: &TreeLine, columns: &TableColumns, width: usize) -> Line<'static> {
        let spans = match line {
            TreeLine::Heading(heading) => heading.spans(self.tick, &self.theme, width),
            TreeLine::Session { row, branch } => self.row_spans(row, *branch, columns),
        };
        if self.is_cursor(line) {
            return self.cursor_line(spans, width);
        }
        if self.is_marked(line) {
            return self.bar(&spans, width);
        }
        Line::from(spans)
    }

    fn is_cursor(&self, line: &TreeLine) -> bool {
        self.selection == Some(&line.selection())
    }

    fn is_marked(&self, line: &TreeLine) -> bool {
        self.marked.contains(&line.selection())
    }

    fn cursor_line(&self, spans: Vec<Span<'static>>, width: usize) -> Line<'static> {
        match self.out_of_focus {
            true => self.underlined(spans, width),
            false => self.bar(&spans, width),
        }
    }

    fn bar(&self, spans: &[Span], width: usize) -> Line<'static> {
        let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
        Line::from(Span::styled(
            format!("{text:<width$}"),
            self.theme.selected(),
        ))
    }

    fn underlined(&self, spans: Vec<Span<'static>>, width: usize) -> Line<'static> {
        let mut line = Line::from(spans);
        let padding = width.saturating_sub(line.width());
        line.push_span(Span::raw(" ".repeat(padding)));
        line.patch_style(self.theme.selected_out_of_focus())
    }

    fn row_spans(
        &self,
        row: &SessionSummary,
        branch: TreeBranch,
        columns: &TableColumns,
    ) -> Vec<Span<'static>> {
        let indicator = StateIndicator::new(row.state(), self.tick);
        let mut spans = vec![
            Span::styled(branch.prefix(), self.theme.dim()),
            Span::styled(indicator.glyph().to_string(), indicator.style(&self.theme)),
        ];
        spans.extend(self.colored_cells(row, columns));
        spans
    }

    fn colored_cells(&self, row: &SessionSummary, columns: &TableColumns) -> Vec<Span<'static>> {
        let cells = Self::cells(row);
        let text = columns.row(&cells);
        let agent = vec![Span::styled(cells.agent().to_string(), self.theme.dim())];
        let mut colored = ColoredRow::new(&text, self.theme.base())
            .skip(&columns.name_cell(row.name()))
            .color_first(cells.agent(), agent);
        if let Some(pull_request) = row.pull_request() {
            colored =
                colored.color_first(cells.pull_request(), self.pull_request_spans(pull_request));
        }
        if let Some(diff) = row.diff() {
            colored = colored.color_last(cells.diff(), self.diff_spans(diff));
        }
        colored.into_spans()
    }

    fn cells(row: &SessionSummary) -> RowCells {
        RowCells::new(row.name())
            .with_agent(Self::agent_text(row))
            .with_pull_request(Self::pull_request_text(row))
            .with_diff(Self::diff_text(row))
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

    fn agent_text(row: &SessionSummary) -> String {
        row.metadata()
            .map_or(String::new(), |metadata| metadata.agent().to_string())
    }

    fn pull_request_text(row: &SessionSummary) -> String {
        row.pull_request()
            .map_or(String::new(), Self::pull_request_cell_text)
    }

    fn pull_request_cell_text(pull_request: &PullRequest) -> String {
        match Self::shows_approval(pull_request) {
            true => format!("{pull_request}{}", Self::APPROVED_CHECK),
            false => pull_request.to_string(),
        }
    }

    fn pull_request_spans(&self, pull_request: &PullRequest) -> Vec<Span<'static>> {
        let style = match Self::shows_approval(pull_request) {
            true => self.theme.approved(),
            false => self.theme.pull_request(pull_request.state()),
        };
        vec![Span::styled(
            Self::pull_request_cell_text(pull_request),
            style,
        )]
    }

    fn shows_approval(pull_request: &PullRequest) -> bool {
        pull_request.state() == PullRequestState::Open && pull_request.is_approved()
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
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::{Color, Modifier};

    use super::*;
    use crate::domain::{DiffStat, PullRequestReview, RepoGroup, SessionState, UnmanagedSession};
    use crate::test_support::ServiceFixture;

    struct TableFixture;

    impl TableFixture {
        fn managed(name: &str, state: SessionState) -> SessionSummary {
            SessionSummary::managed(ServiceFixture::session(name), state, false)
        }

        fn two_repos() -> Vec<SessionSummary> {
            vec![
                ServiceFixture::summary_in("fix-auth", "/repos/alpha", SessionState::Working),
                ServiceFixture::summary_in("dark-mode", "/repos/alpha", SessionState::Waiting),
                ServiceFixture::summary_in("docs", "/repos/beta", SessionState::Exited),
            ]
        }

        fn running(name: &str, agent: &str) -> SessionSummary {
            let session = ServiceFixture::session_with_agent(name, agent);
            SessionSummary::managed(session, SessionState::Waiting, false)
        }

        fn approved(number: u64, state: PullRequestState) -> SessionSummary {
            Self::managed("fix-auth", SessionState::Waiting).with_pull_request(
                ServiceFixture::pull_request(number, state)
                    .with_review(PullRequestReview::Approved),
            )
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
            let expanded = SessionTree::new(rows, &HashSet::new());
            Self::draw(expanded, selection, tick, height)
        }

        fn render_folded(rows: &[SessionSummary], height: u16) -> Terminal<TestBackend> {
            let every_repo = rows.iter().map(RepoGroup::of).collect();
            Self::draw(SessionTree::new(rows, &every_repo), None, 0, height)
        }

        fn draw(
            tree: SessionTree,
            selection: Option<&ListSelection>,
            tick: u64,
            height: u16,
        ) -> Terminal<TestBackend> {
            Self::draw_table(
                SessionTable::new(tree, selection, tick, Theme::default()),
                height,
            )
        }

        fn render_out_of_focus(
            rows: &[SessionSummary],
            cursor: &str,
            marked: &[&str],
        ) -> Terminal<TestBackend> {
            let cursor = ListSelection::Session(cursor.to_string());
            let marked = marked
                .iter()
                .map(|name| ListSelection::Session(name.to_string()))
                .collect();
            let tree = SessionTree::new(rows, &HashSet::new());
            let table = SessionTable::new(tree, Some(&cursor), 0, Theme::default())
                .with_marked(marked)
                .out_of_focus();
            Self::draw_table(table, 6)
        }

        fn draw_table(table: SessionTable, height: u16) -> Terminal<TestBackend> {
            let mut terminal = Terminal::new(TestBackend::new(50, height)).unwrap();
            terminal
                .draw(|frame| table.render(frame, frame.area()))
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

        fn column(terminal: &Terminal<TestBackend>, row: u16, text: &str) -> usize {
            let line = Self::line(terminal, row);
            let byte = line.find(text).unwrap();
            line[..byte].chars().count()
        }

        fn header_starts_over(rows: &[SessionSummary], number: &str) -> bool {
            let terminal = Self::render(rows, None, 0, 6);
            Self::column(&terminal, 1, "PR") == Self::column(&terminal, 2, number)
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
    fn an_approved_pull_request_shows_a_green_checkmark_after_its_number() {
        let row = TableFixture::approved(142, PullRequestState::Open);

        let terminal = TableFixture::render(&[row], None, 0, 6);
        let line = TableFixture::line(&terminal, 2);
        let number = u16::try_from(TableFixture::column(&terminal, 2, "#142")).unwrap();
        let check = u16::try_from(TableFixture::column(&terminal, 2, "✓")).unwrap();
        let buffer = terminal.backend().buffer();

        assert!(line.contains("#142✓"));
        assert_eq!(buffer[(number, 2)].fg, Color::Green);
        assert!(buffer[(number, 2)].modifier.contains(Modifier::BOLD));
        assert_eq!(buffer[(check, 2)].fg, Color::Green);
        assert!(buffer[(check, 2)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn an_unapproved_pull_request_shows_only_its_number() {
        let row = TableFixture::managed("fix-auth", SessionState::Waiting)
            .with_pull_request(ServiceFixture::pull_request(142, PullRequestState::Open));

        let terminal = TableFixture::render(&[row], None, 0, 6);
        let line = TableFixture::line(&terminal, 2);
        let number = u16::try_from(TableFixture::column(&terminal, 2, "#142")).unwrap();
        let buffer = terminal.backend().buffer();

        assert!(line.contains("#142"));
        assert!(!line.contains('✓'));
        assert_eq!(buffer[(number, 2)].fg, Color::Green);
        assert!(!buffer[(number, 2)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn the_pull_request_header_starts_over_an_approved_number() {
        assert!(TableFixture::header_starts_over(
            &[TableFixture::approved(142, PullRequestState::Open)],
            "#142",
        ));
    }

    #[test]
    fn the_pull_request_header_starts_over_an_unapproved_number() {
        let row = TableFixture::managed("fix-auth", SessionState::Waiting)
            .with_pull_request(ServiceFixture::pull_request(142, PullRequestState::Open));

        assert!(TableFixture::header_starts_over(&[row], "#142"));
    }

    #[test]
    fn only_an_open_pull_request_shows_its_approval() {
        let states = [
            PullRequestState::Draft,
            PullRequestState::Merged,
            PullRequestState::Closed,
        ];

        for state in states {
            let terminal = TableFixture::render(&[TableFixture::approved(5, state)], None, 0, 6);
            assert!(!TableFixture::line(&terminal, 2).contains('✓'));
        }
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
    fn an_out_of_focus_cursor_row_is_underlined_instead_of_reversed() {
        let rows = [TableFixture::managed("fix-auth", SessionState::Waiting)];

        let terminal = TableFixture::render_out_of_focus(&rows, "fix-auth", &[]);
        let cell = &terminal.backend().buffer()[(1, 2)];

        assert!(cell.modifier.contains(Modifier::UNDERLINED));
        assert!(!cell.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn an_out_of_focus_underline_spans_the_whole_width() {
        let rows = [TableFixture::managed("fix-auth", SessionState::Waiting)];

        let terminal = TableFixture::render_out_of_focus(&rows, "fix-auth", &[]);
        let last_cell = &terminal.backend().buffer()[(48, 2)];

        assert!(last_cell.modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn an_out_of_focus_cursor_row_keeps_its_colors() {
        let rows = [TableFixture::managed("fix-auth", SessionState::Waiting)];

        let terminal = TableFixture::render_out_of_focus(&rows, "fix-auth", &[]);
        let indicator = &terminal.backend().buffer()[(1, 2)];

        assert_eq!(indicator.fg, Color::Green);
    }

    #[test]
    fn an_out_of_focus_marked_row_stays_reversed_so_only_the_cursor_shows_focus() {
        let rows = [
            TableFixture::managed("dark-mode", SessionState::Waiting),
            TableFixture::managed("fix-auth", SessionState::Waiting),
        ];

        let terminal =
            TableFixture::render_out_of_focus(&rows, "fix-auth", &["dark-mode", "fix-auth"]);
        let cell = &terminal.backend().buffer()[(1, 2)];

        assert!(cell.modifier.contains(Modifier::REVERSED));
        assert!(!cell.modifier.contains(Modifier::UNDERLINED));
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
    fn a_folded_heading_shows_its_number_arrow_and_state_counts() {
        let terminal = TableFixture::render_folded(&TableFixture::two_repos(), 8);

        assert!(TableFixture::line(&terminal, 2).starts_with("│1 ▸ alpha ⠋1 ●1 "));
        assert!(TableFixture::line(&terminal, 3).starts_with("│2 ▸ beta ✗1 "));
    }

    #[test]
    fn an_expanded_heading_shows_only_its_number_arrow_and_repo() {
        let terminal = TableFixture::render(&TableFixture::two_repos(), None, 0, 8);

        let heading = TableFixture::line(&terminal, 2);
        assert_eq!(heading.trim_end_matches(['│', ' ']), "│1 ▾ alpha");
    }

    #[test]
    fn a_heading_dims_its_number_bolds_its_repo_and_colors_its_state_counts() {
        let terminal = TableFixture::render_folded(&TableFixture::two_repos(), 8);
        let buffer = terminal.backend().buffer();

        assert!(buffer[(1, 2)].modifier.contains(Modifier::DIM));
        assert!(buffer[(5, 2)].modifier.contains(Modifier::BOLD));
        assert_eq!(buffer[(14, 2)].fg, Color::Green);
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
    fn the_name_title_lines_up_with_the_repo_names() {
        let terminal = TableFixture::render(&TableFixture::two_repos(), None, 0, 8);

        assert!(TableFixture::line(&terminal, 1).starts_with("│    Name "));
        assert!(TableFixture::line(&terminal, 2).starts_with("│1 ▾ alpha"));
    }

    #[test]
    fn the_titles_after_the_name_stay_over_their_cells() {
        let mut rows = TableFixture::two_repos();
        rows[0] = rows[0].clone().with_diff(DiffStat::from_changes([], 2));

        let terminal = TableFixture::render(&rows, None, 0, 8);

        assert_eq!(
            TableFixture::column(&terminal, 1, "Diff"),
            TableFixture::column(&terminal, 3, "+0 -0 ?2")
        );
    }

    #[test]
    fn sessions_on_several_agents_show_the_agent_after_the_name() {
        let rows = [
            TableFixture::running("fix-auth", "claude"),
            TableFixture::running("docs", "opencode"),
        ];

        let terminal = TableFixture::render(&rows, None, 0, 6);

        assert!(TableFixture::line(&terminal, 1).starts_with("│  Name          Agent     Diff"));
        assert!(TableFixture::line(&terminal, 2).starts_with("│● fix-auth      claude"));
        assert!(TableFixture::line(&terminal, 3).starts_with("│● docs          opencode"));
    }

    #[test]
    fn sessions_on_one_agent_show_no_agent_column() {
        let rows = [
            TableFixture::running("fix-auth", "claude"),
            TableFixture::running("docs", "claude"),
        ];

        let terminal = TableFixture::render(&rows, None, 0, 6);

        assert!(!TableFixture::line(&terminal, 1).contains("Agent"));
        assert!(!TableFixture::line(&terminal, 2).contains("claude"));
    }

    #[test]
    fn only_the_agent_cell_is_dim_when_the_name_contains_the_agent() {
        let rows = [
            TableFixture::running("claude-docs", "claude"),
            TableFixture::running("fix-auth", "opencode"),
        ];

        let terminal = TableFixture::render(&rows, None, 0, 6);
        let buffer = terminal.backend().buffer();

        assert!(!buffer[(3, 2)].modifier.contains(Modifier::DIM));
        assert!(buffer[(17, 2)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn the_minimal_tier_has_no_header() {
        let rows = [TableFixture::managed("fix-auth", SessionState::Waiting)];
        let mut terminal = Terminal::new(TestBackend::new(36, 5)).unwrap();

        terminal
            .draw(|frame| {
                SessionTable::new(
                    SessionTree::new(&rows, &HashSet::new()),
                    None,
                    0,
                    Theme::default(),
                )
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
                SessionTable::new(
                    SessionTree::new(&[], &HashSet::new()),
                    None,
                    0,
                    Theme::default(),
                )
                .render(frame, frame.area())
            })
            .unwrap();

        assert!(TableFixture::line(&terminal, 1).starts_with("│  Name"));
    }
}

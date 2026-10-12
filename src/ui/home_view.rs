use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use super::preview_panel::PreviewPanel;
use super::session_table::SessionTable;
use super::status_bar::StatusBar;

pub struct HomeView<'a> {
    table: SessionTable<'a>,
    preview: PreviewPanel<'a>,
    status: StatusBar<'a>,
}

impl<'a> HomeView<'a> {
    const TOO_SMALL: &'static str = "Terminal too small";
    const STATUS_SHARE: u16 = 4;

    pub fn new(table: SessionTable<'a>, preview: PreviewPanel<'a>, status: StatusBar<'a>) -> Self {
        Self {
            table,
            preview,
            status,
        }
    }

    pub fn render(self, frame: &mut Frame) {
        let area = frame.area();
        let tier = LayoutTier::for_size(area.width, area.height);
        if tier == LayoutTier::TooSmall {
            CenteredText::new([Self::TOO_SMALL]).render(frame, area);
            return;
        }
        let status_room = (area.height / Self::STATUS_SHARE).max(1);
        let status_rows = self.status.rows(area.width).min(status_room);
        let [main, status] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(status_rows)]).areas(area);
        self.status.render(frame, status);
        let table = self.table.for_tier(tier);
        if tier.shows_preview() && table.has_rows() {
            Self::render_side_by_side(table, self.preview, frame, main);
            return;
        }
        table.render(frame, main);
    }

    fn render_side_by_side(
        table: SessionTable,
        preview: PreviewPanel,
        frame: &mut Frame,
        area: Rect,
    ) {
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .areas(area);
        table.render(frame, left);
        preview.render(frame, right);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutTier {
    Full,
    Compact,
    Minimal,
    TooSmall,
}

impl LayoutTier {
    const FULL: (u16, u16) = (80, 24);
    const COMPACT: (u16, u16) = (60, 12);
    const MINIMAL: (u16, u16) = (36, 8);
    const STATUS_BAR: usize = 1;
    const BORDERS: usize = 2;
    const HEADER: usize = 1;

    pub fn for_size(width: u16, height: u16) -> Self {
        let reaches = |(minimum_width, minimum_height): (u16, u16)| {
            width >= minimum_width && height >= minimum_height
        };
        if reaches(Self::FULL) {
            return Self::Full;
        }
        if reaches(Self::COMPACT) {
            return Self::Compact;
        }
        if reaches(Self::MINIMAL) {
            return Self::Minimal;
        }
        Self::TooSmall
    }

    pub fn shows_preview(self) -> bool {
        self == Self::Full
    }

    pub fn table_rows(self, height: u16) -> usize {
        let chrome = match self {
            Self::TooSmall => return 0,
            Self::Minimal => Self::STATUS_BAR + Self::BORDERS,
            Self::Full | Self::Compact => Self::STATUS_BAR + Self::BORDERS + Self::HEADER,
        };
        usize::from(height).saturating_sub(chrome)
    }
}

pub struct CenteredText<'a> {
    lines: Vec<Line<'a>>,
}

impl<'a> CenteredText<'a> {
    pub fn new<L: Into<Line<'a>>>(lines: impl IntoIterator<Item = L>) -> Self {
        Self {
            lines: lines.into_iter().map(Into::into).collect(),
        }
    }

    pub fn fits(&self, area: Rect) -> bool {
        let widest = self.lines.iter().map(Line::width).max().unwrap_or_default();
        widest <= usize::from(area.width) && self.lines.len() <= usize::from(area.height)
    }

    pub fn render(self, frame: &mut Frame, area: Rect) {
        let height = u16::try_from(self.lines.len()).unwrap_or(u16::MAX);
        let [_, middle, _] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(height),
            Constraint::Fill(1),
        ])
        .areas(area);
        frame.render_widget(
            Paragraph::new(self.lines).alignment(Alignment::Center),
            middle,
        );
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::domain::{SessionState, SessionSummary};
    use crate::test_support::ServiceFixture;
    use crate::ui::empty_state::EmptyState;
    use crate::ui::theme::Theme;

    struct ViewFixture;

    impl ViewFixture {
        fn rows() -> Vec<SessionSummary> {
            vec![SessionSummary::managed(
                ServiceFixture::session("fix-auth"),
                SessionState::Waiting,
                false,
            )]
        }

        fn render(rows: &[SessionSummary], width: u16, height: u16) -> String {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let selected = rows.first();
            terminal
                .draw(|frame| {
                    HomeView::new(
                        SessionTable::new(rows, selected.map(SessionSummary::name), 0, Theme),
                        PreviewPanel::new(selected, None, Theme),
                        StatusBar::new(Theme),
                    )
                    .render(frame)
                })
                .unwrap();
            terminal
                .backend()
                .buffer()
                .content()
                .chunks(usize::from(width))
                .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        }
    }

    #[test]
    fn eighty_by_twenty_four_is_the_smallest_full_tier() {
        assert_eq!(LayoutTier::for_size(80, 24), LayoutTier::Full);
        assert_eq!(LayoutTier::for_size(79, 24), LayoutTier::Compact);
        assert_eq!(LayoutTier::for_size(80, 23), LayoutTier::Compact);
    }

    #[test]
    fn the_tiers_scale_down_to_thirty_six_by_eight() {
        assert_eq!(LayoutTier::for_size(60, 12), LayoutTier::Compact);
        assert_eq!(LayoutTier::for_size(59, 12), LayoutTier::Minimal);
        assert_eq!(LayoutTier::for_size(36, 8), LayoutTier::Minimal);
        assert_eq!(LayoutTier::for_size(35, 8), LayoutTier::TooSmall);
        assert_eq!(LayoutTier::for_size(36, 7), LayoutTier::TooSmall);
    }

    #[test]
    fn the_table_shows_the_rows_left_by_the_status_bar_borders_and_header() {
        assert_eq!(LayoutTier::Full.table_rows(28), 24);
        assert_eq!(LayoutTier::Minimal.table_rows(8), 5);
        assert_eq!(LayoutTier::TooSmall.table_rows(7), 0);
    }

    #[test]
    fn the_full_tier_shows_the_preview_beside_the_table() {
        let screen = ViewFixture::render(&ViewFixture::rows(), 100, 28);

        assert!(screen.lines().next().unwrap().contains("┐┌ Preview "));
    }

    #[test]
    fn the_compact_tier_hides_the_preview() {
        let screen = ViewFixture::render(&ViewFixture::rows(), 60, 12);

        assert!(!screen.contains("Preview"));
        assert!(screen.contains("Branch"));
    }

    #[test]
    fn the_minimal_tier_keeps_only_state_and_name() {
        let screen = ViewFixture::render(&ViewFixture::rows(), 36, 8);

        assert!(screen.contains("● fix-auth"));
        assert!(!screen.contains("Branch"));
        assert!(!screen.contains("Name"));
    }

    #[test]
    fn a_terminal_below_the_minimal_tier_shows_only_the_too_small_notice() {
        let screen = ViewFixture::render(&ViewFixture::rows(), 35, 8);

        assert!(screen.contains("Terminal too small"));
        assert!(!screen.contains("fix-auth"));
    }

    #[test]
    fn the_full_tier_without_sessions_invites_the_user_to_create_one() {
        let screen = ViewFixture::render(&[], 100, 28);

        assert_eq!(screen.matches("No sessions yet.").count(), 1);
        assert!(screen.contains("Press n to create one."));
        assert!(screen.contains("  Name"));
    }

    #[test]
    fn the_full_tier_without_sessions_gives_the_table_the_whole_width() {
        let screen = ViewFixture::render(&[], 100, 28);

        assert!(!screen.contains("Preview"));
    }

    #[test]
    fn the_smallest_full_tier_without_sessions_shows_the_logo() {
        let screen = ViewFixture::render(&[], 80, 24);

        assert!(screen.contains(EmptyState::logo_bottom_row()));
    }

    #[test]
    fn the_compact_tier_without_sessions_shows_the_empty_state_in_the_table() {
        let screen = ViewFixture::render(&[], 60, 12);

        assert!(screen.contains("No sessions yet."));
    }

    #[test]
    fn the_compact_tier_without_sessions_shows_the_logo_from_sixty_four_columns() {
        let screen = ViewFixture::render(&[], 64, 12);

        assert!(screen.contains(EmptyState::logo_bottom_row()));
    }
}

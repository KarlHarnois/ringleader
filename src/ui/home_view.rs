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
            CenteredText::new(&[Self::TOO_SMALL]).render(frame, area);
            return;
        }
        let [main, status] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        self.status.render(frame, status);
        let table = self.table.for_tier(tier);
        if tier.shows_preview() {
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
}

pub struct CenteredText<'a> {
    lines: &'a [&'a str],
}

impl<'a> CenteredText<'a> {
    pub const EMPTY_STATE: [&'static str; 2] = ["No sessions yet.", "Press n to create one."];

    pub fn new(lines: &'a [&'a str]) -> Self {
        Self { lines }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let height = u16::try_from(self.lines.len()).unwrap_or(u16::MAX);
        let [_, middle, _] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(height),
            Constraint::Fill(1),
        ])
        .areas(area);
        let lines: Vec<Line> = self.lines.iter().map(|line| Line::from(*line)).collect();
        frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), middle);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::domain::{SessionState, SessionSummary};
    use crate::test_support::ServiceFixture;
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
    fn the_full_tier_shows_the_preview_beside_the_table() {
        let screen = ViewFixture::render(&ViewFixture::rows(), 100, 28);

        assert!(screen.lines().next().unwrap().contains("┐┌ Preview "));
    }

    #[test]
    fn the_compact_tier_hides_the_preview() {
        let screen = ViewFixture::render(&ViewFixture::rows(), 60, 12);

        assert!(!screen.contains("Preview"));
        assert!(screen.contains("ringleader/fix-auth"));
    }

    #[test]
    fn the_minimal_tier_keeps_only_state_and_name() {
        let screen = ViewFixture::render(&ViewFixture::rows(), 36, 8);

        assert!(screen.contains("● fix-auth"));
        assert!(!screen.contains("ringleader/fix-auth"));
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
    fn the_compact_tier_without_sessions_shows_the_empty_state_in_the_table() {
        let screen = ViewFixture::render(&[], 60, 12);

        assert!(screen.contains("No sessions yet."));
    }
}

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::theme::Theme;
use crate::domain::{SessionState, SessionSummary};

pub struct HomeView<'a> {
    rows: &'a [SessionSummary],
    selection: Option<&'a str>,
    tick: u64,
    theme: Theme,
}

impl<'a> HomeView<'a> {
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
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let lines: Vec<Line> = self.rows.iter().map(|row| self.row_line(row)).collect();
        let table =
            Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Sessions "));
        frame.render_widget(table, area);
    }

    fn row_line(&self, row: &SessionSummary) -> Line<'static> {
        let indicator = StateIndicator::new(row.state(), self.tick);
        let selected = self.selection == Some(row.name());
        let name_style = if selected {
            self.theme.selected()
        } else {
            self.theme.base()
        };
        let indicator_style = if selected {
            self.theme.selected()
        } else {
            indicator.style(&self.theme)
        };
        Line::from(vec![
            Span::styled(indicator.glyph().to_string(), indicator_style),
            Span::styled(format!(" {}", row.name()), name_style),
        ])
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
    use ratatui::style::Color;

    use super::*;
    use crate::domain::UnmanagedSession;
    use crate::test_support::ServiceFixture;

    struct ViewFixture;

    impl ViewFixture {
        fn managed(state: SessionState) -> SessionSummary {
            SessionSummary::managed(ServiceFixture::session("fix-auth"), state, false)
        }

        fn render(rows: &[SessionSummary], tick: u64) -> Terminal<TestBackend> {
            let mut terminal = Terminal::new(TestBackend::new(40, 6)).unwrap();
            terminal
                .draw(|frame| HomeView::new(rows, None, tick, Theme).render(frame, frame.area()))
                .unwrap();
            terminal
        }

        fn indicator_cell(terminal: &Terminal<TestBackend>) -> (String, Color) {
            let cell = &terminal.backend().buffer()[(1, 1)];
            (cell.symbol().to_string(), cell.fg)
        }
    }

    #[test]
    fn a_working_row_shows_the_spinner_frame_of_the_tick() {
        let terminal = ViewFixture::render(&[ViewFixture::managed(SessionState::Working)], 2);

        assert_eq!(ViewFixture::indicator_cell(&terminal).0, "⠹");
    }

    #[test]
    fn the_spinner_advances_with_the_tick() {
        let rows = [ViewFixture::managed(SessionState::Working)];

        let first = ViewFixture::indicator_cell(&ViewFixture::render(&rows, 0)).0;
        let second = ViewFixture::indicator_cell(&ViewFixture::render(&rows, 1)).0;

        assert_ne!(first, second);
    }

    #[test]
    fn a_waiting_row_shows_a_green_bullet() {
        let terminal = ViewFixture::render(&[ViewFixture::managed(SessionState::Waiting)], 0);

        assert_eq!(
            ViewFixture::indicator_cell(&terminal),
            ("●".to_string(), Color::Green)
        );
    }

    #[test]
    fn an_exited_row_shows_a_red_cross() {
        let terminal = ViewFixture::render(&[ViewFixture::managed(SessionState::Exited)], 0);

        assert_eq!(
            ViewFixture::indicator_cell(&terminal),
            ("✗".to_string(), Color::Red)
        );
    }

    #[test]
    fn a_stopped_row_shows_a_hollow_circle() {
        let terminal = ViewFixture::render(&[ViewFixture::managed(SessionState::Stopped)], 0);

        assert_eq!(ViewFixture::indicator_cell(&terminal).0, "○");
    }

    #[test]
    fn an_unmanaged_row_shows_a_yellow_question_mark() {
        let stranger = ServiceFixture::session("mystery");
        let row = SessionSummary::unmanaged(UnmanagedSession::new(
            "mystery",
            ServiceFixture::waiting(&stranger).without_metadata(),
        ));

        let terminal = ViewFixture::render(&[row], 0);

        assert_eq!(
            ViewFixture::indicator_cell(&terminal),
            ("?".to_string(), Color::Yellow)
        );
    }
}

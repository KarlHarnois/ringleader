use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::theme::Theme;
use crate::domain::SessionSummary;

pub struct HomeView<'a> {
    rows: &'a [SessionSummary],
    selection: Option<&'a str>,
    theme: Theme,
}

impl<'a> HomeView<'a> {
    pub fn new(rows: &'a [SessionSummary], selection: Option<&'a str>, theme: Theme) -> Self {
        Self {
            rows,
            selection,
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
        let text = format!("{:<9} {}", row.state().label(), row.name());
        let style = if self.selection == Some(row.name()) {
            self.theme.selected()
        } else {
            self.theme.state(row.state())
        };
        Line::from(Span::styled(text, style))
    }
}

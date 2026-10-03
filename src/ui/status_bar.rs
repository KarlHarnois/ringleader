use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::theme::Theme;

pub struct StatusBar<'a> {
    message: Option<&'a str>,
    theme: Theme,
}

impl<'a> StatusBar<'a> {
    const KEYS: [(&'static str, &'static str); 2] = [("Enter", "attach"), ("q", "quit")];
    const GAP: &'static str = "   ";

    pub fn new(message: Option<&'a str>, theme: Theme) -> Self {
        Self { message, theme }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let line = match self.message {
            Some(message) => Line::from(Span::styled(format!(" {message}"), self.theme.error())),
            None => self.key_hints(),
        };
        frame.render_widget(Paragraph::new(line), area);
    }

    fn key_hints(&self) -> Line<'static> {
        let hints: Vec<String> = Self::KEYS
            .iter()
            .map(|(key, action)| format!("{key} {action}"))
            .collect();
        Line::from(format!(" {}", hints.join(Self::GAP)))
    }
}

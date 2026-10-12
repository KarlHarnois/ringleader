use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui_textarea::{TextArea, WrapMode};

use super::dialog_labels::DialogLabels;
use super::new_session_field::NewSessionField;

pub struct PromptField {
    editor: TextArea<'static>,
}

impl Default for PromptField {
    fn default() -> Self {
        let mut editor = TextArea::default();
        editor.set_wrap_mode(WrapMode::WordOrGlyph);
        editor.set_cursor_line_style(Style::default());
        editor.set_cursor_style(Style::default());
        Self { editor }
    }
}

impl PromptField {
    const LINE_BREAK_KEY: char = 'j';
    const CURSOR_COLUMN_WIDTH: u16 = 1;

    pub fn value(&self) -> String {
        self.editor.lines().join("\n")
    }

    pub fn edit(&mut self, key: &KeyEvent) -> bool {
        if Self::breaks_line(key) {
            self.editor.insert_newline();
            return true;
        }
        self.editor.input(*key)
    }

    pub fn paste(&mut self, text: &str) {
        let unix_line_breaks = text.replace("\r\n", "\n").replace('\r', "\n");
        self.editor.insert_str(unix_line_breaks);
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, labels: DialogLabels, cursor_style: Style) {
        let label = labels.labeled(NewSessionField::Prompt.label(), vec![Line::default()]);
        frame.render_widget(Paragraph::new(label), area);
        let field = labels.field_area(area);
        frame.render_widget(&self.editor, Self::text_area(field));
        self.paint_cursor(frame.buffer_mut(), field, cursor_style);
    }

    fn breaks_line(key: &KeyEvent) -> bool {
        key.code == KeyCode::Char(Self::LINE_BREAK_KEY) && key.modifiers == KeyModifiers::CONTROL
    }

    fn text_area(field: Rect) -> Rect {
        let [text, _] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(Self::CURSOR_COLUMN_WIDTH),
        ])
        .areas(field);
        text
    }

    fn paint_cursor(&self, buffer: &mut Buffer, field: Rect, cursor_style: Style) {
        let position = self.cursor_position(Self::text_area(field));
        if field.contains(position) {
            buffer[position].set_style(cursor_style);
        }
    }

    fn cursor_position(&self, text: Rect) -> Position {
        let cursor = self.editor.screen_cursor();
        let (top_row, _) = self.editor.scroll_offset();
        let row = u16::try_from(cursor.row).unwrap_or(u16::MAX);
        let column = u16::try_from(cursor.col).unwrap_or(u16::MAX);
        Position::new(
            text.x.saturating_add(column),
            text.y.saturating_add(row.saturating_sub(top_row)),
        )
    }
}

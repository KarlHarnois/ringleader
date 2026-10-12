use std::cell::RefCell;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph, Widget};
use ratatui_textarea::{CursorMove, TextArea, WrapMode};

use super::dialog_labels::DialogLabels;
use super::new_session_field::NewSessionField;

pub struct PromptField {
    editor: RefCell<TextArea<'static>>,
}

impl Default for PromptField {
    fn default() -> Self {
        let mut editor = TextArea::default();
        editor.set_wrap_mode(WrapMode::WordOrGlyph);
        editor.set_cursor_line_style(Style::default());
        editor.set_cursor_style(Style::default());
        Self {
            editor: RefCell::new(editor),
        }
    }
}

impl PromptField {
    const MOST_ROWS: usize = 6;
    const LINE_BREAK_KEY: char = 'j';
    const CURSOR_COLUMN_WIDTH: u16 = 1;
    const PROBE_HEIGHT: u16 = 1;

    pub fn value(&self) -> String {
        self.editor.borrow().lines().join("\n")
    }

    pub fn edit(&mut self, key: &KeyEvent) -> bool {
        let editor = self.editor.get_mut();
        if Self::breaks_line(key) {
            editor.insert_newline();
            return true;
        }
        editor.input(*key)
    }

    pub fn paste(&mut self, text: &str) {
        let unix_line_breaks = text.replace("\r\n", "\n").replace('\r', "\n");
        self.editor.get_mut().insert_str(unix_line_breaks);
    }

    pub fn rows(&self, field_width: usize) -> usize {
        let width = u16::try_from(field_width).unwrap_or(u16::MAX);
        let field = Rect::new(0, 0, width, Self::PROBE_HEIGHT);
        Self::wrapped_rows(&self.editor.borrow(), Self::text_area(field)).min(Self::MOST_ROWS)
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, labels: DialogLabels, cursor_style: Style) {
        let label = labels.labeled(NewSessionField::Prompt.label(), vec![Line::default()]);
        frame.render_widget(Paragraph::new(label), area);
        let field = labels.field_area(area);
        self.render_text(frame, Self::text_area(field));
        self.paint_cursor(frame.buffer_mut(), field, cursor_style);
    }

    fn breaks_line(key: &KeyEvent) -> bool {
        key.code == KeyCode::Char(Self::LINE_BREAK_KEY) && key.modifiers == KeyModifiers::CONTROL
    }

    fn render_text(&self, frame: &mut Frame, text: Rect) {
        let mut editor = self.editor.borrow_mut();
        frame.render_widget(&*editor, text);
        let stale_rows = Self::stale_scroll_rows(&editor, text);
        if stale_rows > 0 {
            Self::scroll_back(&mut editor, stale_rows);
            frame.render_widget(Clear, text);
            frame.render_widget(&*editor, text);
        }
    }

    fn stale_scroll_rows(editor: &TextArea<'static>, text: Rect) -> u16 {
        let (top_row, _) = editor.scroll_offset();
        let wrapped_rows = u16::try_from(Self::wrapped_rows(editor, text)).unwrap_or(u16::MAX);
        let blank_rows_below = top_row
            .saturating_add(text.height)
            .saturating_sub(wrapped_rows);
        blank_rows_below.min(top_row)
    }

    fn scroll_back(editor: &mut TextArea<'static>, rows: u16) {
        let cursor = editor.cursor();
        editor.scroll((-i16::try_from(rows).unwrap_or(i16::MAX), 0));
        if editor.cursor() != cursor {
            editor.move_cursor(CursorMove::Forward);
        }
    }

    fn wrapped_rows(editor: &TextArea<'static>, text: Rect) -> usize {
        let mut probe = editor.clone();
        (&probe).render(text, &mut Buffer::empty(text));
        probe.move_cursor(CursorMove::Bottom);
        probe.move_cursor(CursorMove::End);
        probe.screen_cursor().row + 1
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
        let editor = self.editor.borrow();
        let cursor = editor.screen_cursor();
        let (top_row, _) = editor.scroll_offset();
        let row = u16::try_from(cursor.row).unwrap_or(u16::MAX);
        let column = u16::try_from(cursor.col).unwrap_or(u16::MAX);
        Position::new(
            text.x.saturating_add(column),
            text.y.saturating_add(row.saturating_sub(top_row)),
        )
    }
}

use std::cell::RefCell;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Clear, Widget};
use ratatui_textarea::{CursorMove, DataCursor, TextArea, WrapMode};

use crate::ui::theme::Theme;

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
    const LINE_BREAK_KEY: char = 'j';
    const DELETE_TO_LINE_START_KEY: char = 'u';
    const CURSOR_COLUMN_WIDTH: u16 = 1;
    const PROBE_HEIGHT: u16 = 1;

    pub fn value(&self) -> String {
        self.editor.borrow().lines().join("\n")
    }

    pub fn edit(&mut self, key: &KeyEvent) -> bool {
        let editor = self.editor.get_mut();
        if Self::is_control(key, Self::LINE_BREAK_KEY) {
            editor.insert_newline();
            return true;
        }
        if Self::is_control(key, Self::DELETE_TO_LINE_START_KEY) {
            return editor.delete_line_by_head();
        }
        editor.input(*key)
    }

    pub fn paste(&mut self, text: &str) {
        let unix_line_breaks = text.replace("\r\n", "\n").replace('\r', "\n");
        self.editor.get_mut().insert_str(unix_line_breaks);
    }

    pub fn rows(&self, width: usize) -> usize {
        let inner = Rect {
            width: u16::try_from(width).unwrap_or(u16::MAX),
            height: Self::PROBE_HEIGHT,
            ..Rect::default()
        };
        Self::wrapped_rows(&self.editor.borrow(), Self::text_area(inner))
    }

    pub fn render(&self, frame: &mut Frame, inner: Rect, style: PromptStyle) {
        frame.buffer_mut().set_style(inner, style.text);
        self.render_text(frame, Self::text_area(inner), style);
        self.paint_cursor(frame.buffer_mut(), inner, self.cursor_style(style));
    }

    fn is_control(key: &KeyEvent, character: char) -> bool {
        key.code == KeyCode::Char(character) && key.modifiers == KeyModifiers::CONTROL
    }

    fn render_text(&self, frame: &mut Frame, text: Rect, style: PromptStyle) {
        let mut editor = self.editor.borrow_mut();
        editor.set_style(style.text);
        editor.set_selection_style(style.selection);
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
        Self::put_cursor_back(editor, cursor);
    }

    fn put_cursor_back(editor: &mut TextArea<'static>, cursor: DataCursor) {
        let DataCursor(row, column) = cursor;
        let line_end = editor.lines()[row].chars().count();
        if column == line_end {
            editor.move_cursor(CursorMove::End);
            return;
        }
        let DataCursor(_, pulled_back_to) = editor.cursor();
        for _ in pulled_back_to..column {
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

    fn text_area(inner: Rect) -> Rect {
        Rect {
            width: inner.width.saturating_sub(Self::CURSOR_COLUMN_WIDTH),
            ..inner
        }
    }

    fn cursor_style(&self, style: PromptStyle) -> Style {
        if self.cursor_is_selected() {
            style.cursor_over_selection()
        } else {
            style.cursor
        }
    }

    fn cursor_is_selected(&self) -> bool {
        let editor = self.editor.borrow();
        let DataCursor(row, column) = editor.cursor();
        editor
            .selection_range()
            .is_some_and(|(start, end)| start == (row, column) && start != end)
    }

    fn paint_cursor(&self, buffer: &mut Buffer, inner: Rect, cursor_style: Style) {
        let position = self.cursor_position(Self::text_area(inner));
        if inner.contains(position) {
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

#[derive(Debug, Clone, Copy)]
pub struct PromptStyle {
    text: Style,
    cursor: Style,
    selection: Style,
}

impl PromptStyle {
    pub fn focused(theme: &Theme) -> Self {
        Self {
            text: theme.prompt(),
            cursor: theme.prompt_cursor(),
            selection: theme.selected_text(),
        }
    }

    pub fn unfocused(theme: &Theme) -> Self {
        Self {
            text: theme.prompt(),
            cursor: theme.base(),
            selection: theme.base(),
        }
    }

    fn cursor_over_selection(self) -> Style {
        self.cursor.patch(self.selection)
    }
}

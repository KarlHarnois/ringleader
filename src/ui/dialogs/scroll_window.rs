use std::cell::Cell;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::Line;

#[derive(Default)]
pub struct ScrollWindow {
    offset: usize,
    max_offset: Cell<usize>,
}

impl ScrollWindow {
    const HINT: &'static str = "↑ ↓ scroll · ";

    pub fn scroll_for(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.offset = (self.visible_offset() + 1).min(self.max_offset.get());
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.offset = self.visible_offset().saturating_sub(1);
            }
            _ => {}
        }
    }

    pub fn window(&self, content: Vec<Line<'static>>, room: usize) -> Vec<Line<'static>> {
        self.max_offset.set(content.len().saturating_sub(room));
        content
            .into_iter()
            .skip(self.visible_offset())
            .take(room)
            .collect()
    }

    pub fn hint(&self, close_hint: &str) -> String {
        if self.scrolls() {
            Self::scrolling_hint(close_hint)
        } else {
            close_hint.to_string()
        }
    }

    pub fn scrolling_hint(close_hint: &str) -> String {
        format!("{}{close_hint}", Self::HINT)
    }

    fn scrolls(&self) -> bool {
        self.max_offset.get() > 0
    }

    fn visible_offset(&self) -> usize {
        self.offset.min(self.max_offset.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct WindowFixture;

    impl WindowFixture {
        fn lines(count: usize) -> Vec<Line<'static>> {
            (0..count).map(|row| Line::from(row.to_string())).collect()
        }
    }

    #[test]
    fn content_one_line_too_long_offers_the_scroll_keys() {
        let scroll = ScrollWindow::default();

        scroll.window(WindowFixture::lines(3), 2);

        assert_eq!(scroll.hint("q close"), "↑ ↓ scroll · q close");
    }

    #[test]
    fn content_that_fits_offers_only_the_close_keys() {
        let scroll = ScrollWindow::default();

        scroll.window(WindowFixture::lines(2), 2);

        assert_eq!(scroll.hint("q close"), "q close");
    }
}

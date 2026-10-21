use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::Line;

use crate::ui::theme::Theme;

#[derive(Debug, Default)]
pub struct MatchList {
    matches: Vec<usize>,
    cursor: usize,
}

impl MatchList {
    pub fn replace(&mut self, matches: Vec<usize>) {
        self.matches = matches;
        self.cursor = 0;
    }

    pub fn move_cursor(&mut self, key: &KeyEvent) -> bool {
        let Some(offset) = Self::cursor_offset(key) else {
            return false;
        };
        let last = self.matches.len().saturating_sub(1);
        self.cursor = self.cursor.saturating_add_signed(offset).min(last);
        true
    }

    pub fn selected(&self) -> Option<usize> {
        self.matches.get(self.cursor).copied()
    }

    pub fn len(&self) -> usize {
        self.matches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    pub fn lines<T: Into<Line<'static>>>(
        &self,
        rows: usize,
        theme: &Theme,
        shown: impl Fn(usize) -> T,
    ) -> Vec<Line<'static>> {
        let first = (self.cursor + 1).saturating_sub(rows);
        let visible = self.matches.iter().enumerate().skip(first).take(rows);
        visible
            .map(|(position, &index)| {
                let style = if position == self.cursor {
                    theme.selected()
                } else {
                    theme.base()
                };
                shown(index).into().patch_style(style)
            })
            .collect()
    }

    fn cursor_offset(key: &KeyEvent) -> Option<isize> {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Up => Some(-1),
            KeyCode::Down => Some(1),
            KeyCode::Char('k' | 'p') if control => Some(-1),
            KeyCode::Char('j' | 'n') if control => Some(1),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::text::Span;

    use super::*;

    struct ListFixture;

    impl ListFixture {
        fn holding(matches: &[usize]) -> MatchList {
            let mut list = MatchList::default();
            list.replace(matches.to_vec());
            list
        }

        fn press(list: &mut MatchList, code: KeyCode, modifiers: KeyModifiers) -> bool {
            list.move_cursor(&KeyEvent::new(code, modifiers))
        }

        fn press_times(list: &mut MatchList, code: KeyCode, times: usize) {
            for _ in 0..times {
                Self::press(list, code, KeyModifiers::NONE);
            }
        }
    }

    #[test]
    fn the_cursor_starts_on_the_first_match() {
        assert_eq!(ListFixture::holding(&[4, 2]).selected(), Some(4));
    }

    #[test]
    fn the_cursor_stops_at_the_last_match() {
        let mut list = ListFixture::holding(&[4, 2]);

        ListFixture::press_times(&mut list, KeyCode::Down, 5);

        assert_eq!(list.selected(), Some(2));
    }

    #[test]
    fn the_cursor_stops_at_the_first_match() {
        let mut list = ListFixture::holding(&[4, 2]);

        ListFixture::press_times(&mut list, KeyCode::Up, 5);

        assert_eq!(list.selected(), Some(4));
    }

    #[test]
    fn control_n_and_control_p_move_like_down_and_up() {
        let mut list = ListFixture::holding(&[4, 2, 7]);

        ListFixture::press(&mut list, KeyCode::Char('n'), KeyModifiers::CONTROL);
        ListFixture::press(&mut list, KeyCode::Char('n'), KeyModifiers::CONTROL);
        ListFixture::press(&mut list, KeyCode::Char('p'), KeyModifiers::CONTROL);

        assert_eq!(list.selected(), Some(2));
    }

    #[test]
    fn a_plain_letter_is_not_a_cursor_key() {
        let mut list = ListFixture::holding(&[4, 2]);

        let moved = ListFixture::press(&mut list, KeyCode::Char('j'), KeyModifiers::NONE);

        assert!(!moved);
        assert_eq!(list.selected(), Some(4));
    }

    #[test]
    fn replacing_the_matches_moves_the_cursor_back_to_the_first() {
        let mut list = ListFixture::holding(&[4, 2]);
        ListFixture::press(&mut list, KeyCode::Down, KeyModifiers::NONE);

        list.replace(vec![9, 8]);

        assert_eq!(list.selected(), Some(9));
    }

    #[test]
    fn the_row_under_the_cursor_is_highlighted() {
        let mut list = ListFixture::holding(&[4, 2]);
        ListFixture::press(&mut list, KeyCode::Down, KeyModifiers::NONE);

        let lines = list.lines(2, &Theme, |index| index.to_string());

        assert_eq!(lines[0].style, Theme.base());
        assert_eq!(lines[1].style, Theme.selected());
    }

    #[test]
    fn a_rows_own_span_styles_survive_the_highlight() {
        let list = ListFixture::holding(&[4]);

        let lines = list.lines(1, &Theme, |index| {
            Line::from(Span::styled(index.to_string(), Theme.emphasis()))
        });

        assert_eq!(lines[0].style, Theme.selected());
        assert_eq!(lines[0].spans[0].style, Theme.emphasis());
    }

    #[test]
    fn no_matches_select_nothing() {
        assert_eq!(ListFixture::holding(&[]).selected(), None);
    }
}

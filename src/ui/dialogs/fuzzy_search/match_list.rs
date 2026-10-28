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
                let line = shown(index).into();
                if position == self.cursor {
                    Self::selected_line(line, theme)
                } else {
                    line.patch_style(theme.base())
                }
            })
            .collect()
    }

    fn selected_line(mut line: Line<'static>, theme: &Theme) -> Line<'static> {
        for span in &mut line.spans {
            span.style = theme.on_selection(span.style);
        }
        line.patch_style(theme.selected())
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

        fn faint(index: usize) -> Line<'static> {
            Line::from(Span::styled(index.to_string(), Theme::default().dim()))
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

        let lines = list.lines(2, &Theme::default(), |index| index.to_string());

        assert_eq!(lines[0].style, Theme::default().base());
        assert_eq!(lines[1].style, Theme::default().selected());
    }

    #[test]
    fn a_rows_own_span_styles_survive_the_highlight() {
        let list = ListFixture::holding(&[4]);

        let lines = list.lines(1, &Theme::default(), |index| {
            Line::from(Span::styled(index.to_string(), Theme::default().emphasis()))
        });

        assert_eq!(lines[0].style, Theme::default().selected());
        assert_eq!(lines[0].spans[0].style, Theme::default().emphasis());
    }

    #[test]
    fn the_selected_row_drops_faint_text_because_faint_fades_into_the_reversed_bar() {
        let list = ListFixture::holding(&[4]);

        let lines = list.lines(1, &Theme::default(), ListFixture::faint);

        assert_eq!(lines[0].spans[0].style, Theme::default().base());
    }

    #[test]
    fn an_unselected_row_keeps_its_faint_text() {
        let list = ListFixture::holding(&[4, 2]);

        let lines = list.lines(2, &Theme::default(), ListFixture::faint);

        assert_eq!(lines[1].spans[0].style, Theme::default().dim());
    }

    #[test]
    fn no_matches_select_nothing() {
        assert_eq!(ListFixture::holding(&[]).selected(), None);
    }
}

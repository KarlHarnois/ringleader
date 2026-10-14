use std::ops::Range;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    top: usize,
    room: usize,
}

impl Viewport {
    pub fn with_room(self, room: usize) -> Self {
        Self { room, ..self }
    }

    pub fn scrolled_to(self, selected: usize, line_count: usize) -> Self {
        let lowest_top_showing_the_selection = (selected + 1).saturating_sub(self.room);
        let highest_top_filling_the_room = line_count.saturating_sub(self.room);
        let top = self
            .top
            .min(selected)
            .max(lowest_top_showing_the_selection)
            .min(highest_top_filling_the_room);
        Self { top, ..self }
    }

    pub fn lines(self, line_count: usize) -> Range<usize> {
        let start = self.top.min(line_count);
        let end = self.top.saturating_add(self.room).min(line_count);
        start..end
    }

    pub fn high(self, line_count: usize) -> Option<usize> {
        self.lines(line_count).next()
    }

    pub fn middle(self, line_count: usize) -> Option<usize> {
        let mut lines = self.lines(line_count);
        let middle = lines.len().saturating_sub(1) / 2;
        lines.nth(middle)
    }

    pub fn low(self, line_count: usize) -> Option<usize> {
        self.lines(line_count).next_back()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ViewportFixture;

    impl ViewportFixture {
        const ROOM: usize = 3;
        const LINE_COUNT: usize = 10;

        fn showing_lines_three_to_five() -> Viewport {
            Viewport::default()
                .with_room(Self::ROOM)
                .scrolled_to(5, Self::LINE_COUNT)
        }

        fn lines_after_selecting(viewport: Viewport, selected: usize) -> Range<usize> {
            viewport
                .scrolled_to(selected, Self::LINE_COUNT)
                .lines(Self::LINE_COUNT)
        }
    }

    #[test]
    fn a_selection_inside_the_view_keeps_it() {
        let viewport = ViewportFixture::showing_lines_three_to_five();

        assert_eq!(ViewportFixture::lines_after_selecting(viewport, 4), 3..6);
    }

    #[test]
    fn a_selection_below_the_view_scrolls_it_until_the_selection_is_the_last_line() {
        let viewport = ViewportFixture::showing_lines_three_to_five();

        assert_eq!(ViewportFixture::lines_after_selecting(viewport, 7), 5..8);
    }

    #[test]
    fn a_selection_above_the_view_scrolls_it_until_the_selection_is_the_first_line() {
        let viewport = ViewportFixture::showing_lines_three_to_five();

        assert_eq!(ViewportFixture::lines_after_selecting(viewport, 1), 1..4);
    }

    #[test]
    fn a_shrunken_list_still_fills_the_room() {
        let viewport = Viewport::default().with_room(3).scrolled_to(9, 10);

        assert_eq!(viewport.scrolled_to(7, 8).lines(8), 5..8);
    }

    #[test]
    fn high_middle_and_low_are_the_first_middle_and_last_shown_lines() {
        let viewport = ViewportFixture::showing_lines_three_to_five();
        let line_count = ViewportFixture::LINE_COUNT;

        assert_eq!(viewport.high(line_count), Some(3));
        assert_eq!(viewport.middle(line_count), Some(4));
        assert_eq!(viewport.low(line_count), Some(5));
    }

    #[test]
    fn a_short_list_puts_the_middle_and_low_lines_among_the_shown_lines() {
        let viewport = Viewport::default().with_room(10);

        assert_eq!(viewport.middle(4), Some(1));
        assert_eq!(viewport.low(4), Some(3));
    }

    #[test]
    fn no_room_shows_no_line_to_select() {
        let viewport = Viewport::default();

        assert_eq!(viewport.high(10), None);
        assert_eq!(viewport.middle(10), None);
        assert_eq!(viewport.low(10), None);
    }
}

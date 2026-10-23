use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::home_view::CenteredText;
use crate::ui::text_fit::TextFit;

pub struct EmptyState {
    message: &'static [&'static str],
}

impl EmptyState {
    const LOGO: [&'static str; 5] = [
        r"    ____  _____   __________    _________    ____  __________",
        r"   / __ \/  _/ | / / ____/ /   / ____/   |  / __ \/ ____/ __ \",
        r"  / /_/ // //  |/ / / __/ /   / __/ / /| | / / / / __/ / /_/ /",
        r" / _, _// // /|  / /_/ / /___/ /___/ ___ |/ /_/ / /___/ _, _/",
        r"/_/ |_/___/_/ |_/\____/_____/_____/_/  |_/_____/_____/_/ |_|",
    ];
    const MESSAGE: [&'static str; 2] = ["No sessions yet.", "Press n to create one."];
    const FAVORITES_MESSAGE: [&'static str; 2] = [
        "No favorite sessions yet.",
        "Press Ctrl-F to show every session.",
    ];

    pub fn all() -> Self {
        Self::for_message(&Self::MESSAGE)
    }

    pub fn favorites() -> Self {
        Self::for_message(&Self::FAVORITES_MESSAGE)
    }

    fn for_message(message: &'static [&'static str]) -> Self {
        Self { message }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let logo_above_message = CenteredText::new(self.logo_above_message());
        if logo_above_message.fits(area) {
            logo_above_message.render(frame, area);
        } else {
            CenteredText::new(self.message.to_vec()).render(frame, area);
        }
    }

    #[cfg(test)]
    pub fn logo_bottom_row() -> &'static str {
        Self::LOGO[Self::LOGO.len() - 1]
    }

    fn logo_above_message(&self) -> Vec<Line<'static>> {
        Self::logo_rows_of_one_width()
            .chain([Line::default()])
            .chain(self.message.iter().map(|line| Line::from(*line)))
            .collect()
    }

    fn logo_rows_of_one_width() -> impl Iterator<Item = Line<'static>> {
        let width = Self::LOGO
            .map(|row| row.chars().count())
            .into_iter()
            .max()
            .unwrap_or_default();
        Self::LOGO
            .into_iter()
            .map(move |row| Line::from(TextFit::new(row).padded(width)))
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    struct ScreenFixture;

    impl ScreenFixture {
        const ODD_SPARE_COLUMNS: u16 = 19;
        const SPARE_ROWS: u16 = 2;

        fn block_size(state: &EmptyState) -> (u16, u16) {
            let block = state.logo_above_message();
            let width = block.iter().map(Line::width).max().unwrap();
            (
                u16::try_from(width).unwrap(),
                u16::try_from(block.len()).unwrap(),
            )
        }

        fn rows_with_room(state: &EmptyState) -> Vec<String> {
            let (width, height) = Self::block_size(state);
            Self::rows(
                state,
                width + Self::ODD_SPARE_COLUMNS,
                height + Self::SPARE_ROWS,
            )
        }

        fn rows(state: &EmptyState, width: u16, height: u16) -> Vec<String> {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| state.render(frame, frame.area()))
                .unwrap();
            terminal
                .backend()
                .buffer()
                .content()
                .chunks(usize::from(width))
                .map(|row| row.iter().map(|cell| cell.symbol()).collect())
                .collect()
        }

        fn shows_logo(rows: &[String]) -> bool {
            rows.iter().any(|row| row.contains(EmptyState::LOGO[1]))
        }

        fn logo_top_left(rows: &[String]) -> (usize, usize) {
            let top = rows
                .iter()
                .position(|row| row.contains(EmptyState::LOGO[0]))
                .unwrap();
            (top, rows[top].find(EmptyState::LOGO[0]).unwrap())
        }

        fn drawn_rows(rows: &[String]) -> Vec<&str> {
            rows.iter()
                .map(|row| row.trim())
                .filter(|row| !row.is_empty())
                .collect()
        }
    }

    #[test]
    fn the_logo_rows_start_in_the_same_column() {
        let state = EmptyState::all();
        let rows = ScreenFixture::rows_with_room(&state);

        let (top, indent) = ScreenFixture::logo_top_left(&rows);
        for (row, expected) in rows[top..].iter().zip(EmptyState::LOGO) {
            assert_eq!(&row[indent..indent + expected.len()], expected);
        }
    }

    #[test]
    fn the_logo_and_message_are_centered_in_the_area() {
        let state = EmptyState::all();
        let rows = ScreenFixture::rows_with_room(&state);

        let (top, left) = ScreenFixture::logo_top_left(&rows);
        let bottom = usize::from(ScreenFixture::SPARE_ROWS) - top;
        let right = usize::from(ScreenFixture::ODD_SPARE_COLUMNS) - left;
        assert!(
            top.abs_diff(bottom) <= 1 && left.abs_diff(right) <= 1,
            "margins: top {top}, bottom {bottom}, left {left}, right {right}"
        );
    }

    #[test]
    fn the_message_stays_under_the_logo() {
        let state = EmptyState::all();
        let rows = ScreenFixture::rows_with_room(&state);

        let logo_bottom = rows
            .iter()
            .position(|row| row.contains(EmptyState::logo_bottom_row()));
        let message = rows.iter().position(|row| row.contains("No sessions yet."));
        assert_eq!(message, logo_bottom.map(|bottom| bottom + 2));
    }

    #[test]
    fn the_favorites_message_names_the_filter_key() {
        let state = EmptyState::favorites();
        let rows = ScreenFixture::rows_with_room(&state);

        assert!(ScreenFixture::drawn_rows(&rows).contains(&"Press Ctrl-F to show every session."));
    }

    #[test]
    fn an_area_exactly_the_size_of_the_logo_and_message_shows_both() {
        let state = EmptyState::all();
        let (width, height) = ScreenFixture::block_size(&state);
        let rows = ScreenFixture::rows(&state, width, height);

        assert!(ScreenFixture::shows_logo(&rows));
        assert!(rows.last().unwrap().contains("Press n to create one."));
    }

    #[test]
    fn an_area_narrower_than_the_logo_shows_only_the_message() {
        let state = EmptyState::all();
        let (width, height) = ScreenFixture::block_size(&state);
        let rows = ScreenFixture::rows(&state, width - 1, height);

        assert_eq!(ScreenFixture::drawn_rows(&rows), EmptyState::MESSAGE);
    }

    #[test]
    fn an_area_too_short_for_the_logo_and_message_shows_only_the_message() {
        let state = EmptyState::all();
        let (width, height) = ScreenFixture::block_size(&state);
        let rows = ScreenFixture::rows(&state, width, height - 1);

        assert_eq!(ScreenFixture::drawn_rows(&rows), EmptyState::MESSAGE);
    }
}

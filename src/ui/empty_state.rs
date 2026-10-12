use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::home_view::CenteredText;

pub struct EmptyState;

impl EmptyState {
    const LOGO: [&'static str; 5] = [
        r"    ____  _____   __________    _________    ____  __________",
        r"   / __ \/  _/ | / / ____/ /   / ____/   |  / __ \/ ____/ __ \",
        r"  / /_/ // //  |/ / / __/ /   / __/ / /| | / / / / __/ / /_/ /",
        r" / _, _// // /|  / /_/ / /___/ /___/ ___ |/ /_/ / /___/ _, _/",
        r"/_/ |_/___/_/ |_/\____/_____/_____/_/  |_/_____/_____/_/ |_|",
    ];
    const MESSAGE: [&'static str; 2] = ["No sessions yet.", "Press n to create one."];

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let logo_above_message = CenteredText::new(Self::logo_above_message());
        if logo_above_message.fits(area) {
            logo_above_message.render(frame, area);
        } else {
            CenteredText::new(Self::MESSAGE).render(frame, area);
        }
    }

    #[cfg(test)]
    pub fn logo_bottom_row() -> &'static str {
        Self::LOGO[Self::LOGO.len() - 1]
    }

    fn logo_above_message() -> Vec<Line<'static>> {
        Self::logo_rows_of_one_width()
            .chain([Line::default()])
            .chain(Self::MESSAGE.map(Line::from))
            .collect()
    }

    fn logo_rows_of_one_width() -> impl Iterator<Item = Line<'static>> {
        let width = Self::LOGO
            .map(str::len)
            .into_iter()
            .max()
            .unwrap_or_default();
        Self::LOGO
            .into_iter()
            .map(move |row| Line::from(format!("{row:<width$}")))
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    struct ScreenFixture;

    impl ScreenFixture {
        fn rows(width: u16, height: u16) -> Vec<String> {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| EmptyState.render(frame, frame.area()))
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

        fn drawn_rows(rows: &[String]) -> Vec<&str> {
            rows.iter()
                .map(|row| row.trim())
                .filter(|row| !row.is_empty())
                .collect()
        }
    }

    #[test]
    fn the_logo_rows_start_in_the_same_column() {
        let rows = ScreenFixture::rows(81, 10);

        let top = rows
            .iter()
            .position(|row| row.contains(EmptyState::LOGO[0]))
            .unwrap();
        let indent = rows[top].find(EmptyState::LOGO[0]).unwrap();
        for (row, expected) in rows[top..].iter().zip(EmptyState::LOGO) {
            assert_eq!(&row[indent..indent + expected.len()], expected);
        }
    }

    #[test]
    fn the_message_stays_under_the_logo() {
        let rows = ScreenFixture::rows(80, 10);

        let logo_bottom = rows
            .iter()
            .position(|row| row.contains(EmptyState::LOGO[4]));
        let message = rows.iter().position(|row| row.contains("No sessions yet."));
        assert_eq!(message, logo_bottom.map(|bottom| bottom + 2));
    }

    #[test]
    fn an_area_exactly_the_size_of_the_logo_and_message_shows_both() {
        let rows = ScreenFixture::rows(62, 8);

        assert!(ScreenFixture::shows_logo(&rows));
        assert!(rows[7].contains("Press n to create one."));
    }

    #[test]
    fn an_area_narrower_than_the_logo_shows_only_the_message() {
        let rows = ScreenFixture::rows(61, 8);

        assert_eq!(ScreenFixture::drawn_rows(&rows), EmptyState::MESSAGE);
    }

    #[test]
    fn an_area_too_short_for_the_logo_and_message_shows_only_the_message() {
        let rows = ScreenFixture::rows(80, 7);

        assert_eq!(ScreenFixture::drawn_rows(&rows), EmptyState::MESSAGE);
    }
}

use ansi_to_tui::IntoText;
use ratatui::text::Line;

use crate::domain::TerminalText;

pub struct PreviewText<'a> {
    raw: &'a str,
}

impl<'a> PreviewText<'a> {
    pub fn new(raw: &'a str) -> Self {
        Self { raw }
    }

    pub fn bottom_lines(&self, room: usize) -> Vec<Line<'static>> {
        let mut lines = self.styled_lines().unwrap_or_else(|| self.plain_lines());
        while lines.last().is_some_and(Self::is_blank) {
            lines.pop();
        }
        let mut lines = Self::squeezed(lines, room);
        let shown = lines.split_off(lines.len().saturating_sub(room));
        let padding = room.saturating_sub(shown.len());
        std::iter::repeat_n(Line::default(), padding)
            .chain(shown)
            .collect()
    }

    fn squeezed(lines: Vec<Line<'static>>, room: usize) -> Vec<Line<'static>> {
        let mut runs = Self::blank_runs(&lines);
        let mut excess = lines.len().saturating_sub(room);
        while excess > 0 {
            let shrinkable = runs.iter_mut().filter(|run| run.kept > 1);
            let Some(longest) = shrinkable.max_by_key(|run| run.kept) else {
                break;
            };
            longest.kept -= 1;
            excess -= 1;
        }
        lines
            .into_iter()
            .enumerate()
            .filter(|(index, _)| runs.iter().all(|run| run.keeps(*index)))
            .map(|(_, line)| line)
            .collect()
    }

    fn blank_runs(lines: &[Line]) -> Vec<BlankRun> {
        let mut runs: Vec<BlankRun> = Vec::new();
        let blank_rows = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| Self::is_blank(line));
        for (index, _) in blank_rows {
            match runs.last_mut() {
                Some(run) if run.end() == index => run.extend(),
                _ => runs.push(BlankRun::starting_at(index)),
            }
        }
        runs
    }

    fn styled_lines(&self) -> Option<Vec<Line<'static>>> {
        self.raw.as_bytes().into_text().ok().map(|text| text.lines)
    }

    fn plain_lines(&self) -> Vec<Line<'static>> {
        TerminalText::new(self.raw)
            .plain()
            .lines()
            .map(|row| Line::from(row.to_string()))
            .collect()
    }

    fn is_blank(line: &Line) -> bool {
        line.spans.iter().all(|span| span.content.trim().is_empty())
    }
}

struct BlankRun {
    start: usize,
    length: usize,
    kept: usize,
}

impl BlankRun {
    fn starting_at(start: usize) -> Self {
        Self {
            start,
            length: 1,
            kept: 1,
        }
    }

    fn end(&self) -> usize {
        self.start + self.length
    }

    fn extend(&mut self) {
        self.length += 1;
        self.kept += 1;
    }

    fn keeps(&self, index: usize) -> bool {
        index < self.start + self.kept || index >= self.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TextFixture;

    impl TextFixture {
        fn shown(rows: &[String], room: usize) -> Vec<String> {
            let screen = rows.join("\n");
            PreviewText::new(&screen)
                .bottom_lines(room)
                .iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect()
                })
                .collect()
        }

        fn rows(rows: &[&str]) -> Vec<String> {
            rows.iter().map(|row| row.to_string()).collect()
        }

        fn with_gap(top: &str, gap: usize, bottom: &str) -> Vec<String> {
            let mut rows = vec![top.to_string()];
            rows.extend(std::iter::repeat_n(String::new(), gap));
            rows.push(bottom.to_string());
            rows
        }
    }

    #[test]
    fn a_full_screen_app_taller_than_the_room_keeps_its_top_and_bottom_rows() {
        let rows = TextFixture::with_gap("reply at the top", 30, "input box at the bottom");

        let shown = TextFixture::shown(&rows, 5);

        assert_eq!(
            shown,
            ["reply at the top", "", "", "", "input box at the bottom"]
        );
    }

    #[test]
    fn the_longest_blank_gap_shrinks_first() {
        let rows = TextFixture::rows(&["a", "", "", "b", "", "", "", "", "", "", "c"]);

        let shown = TextFixture::shown(&rows, 7);

        assert_eq!(shown, ["a", "", "", "b", "", "", "c"]);
    }

    #[test]
    fn a_screen_that_fits_keeps_every_blank_row() {
        let rows = TextFixture::rows(&["a", "", "", "b"]);

        let shown = TextFixture::shown(&rows, 6);

        assert_eq!(shown, ["", "", "a", "", "", "b"]);
    }

    #[test]
    fn content_that_still_does_not_fit_shows_its_last_rows() {
        let rows = TextFixture::rows(&["one", "", "two", "", "three", "", "four"]);

        let shown = TextFixture::shown(&rows, 3);

        assert_eq!(shown, ["three", "", "four"]);
    }
}

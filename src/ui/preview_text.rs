use ansi_to_tui::IntoText;
use ratatui::text::Line;

use crate::domain::TerminalText;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewText {
    lines: Vec<Line<'static>>,
}

impl PreviewText {
    pub fn parse(raw: &str) -> Self {
        let mut lines = Self::styled_lines(raw).unwrap_or_else(|| Self::plain_lines(raw));
        while lines.last().is_some_and(Self::is_blank) {
            lines.pop();
        }
        Self { lines }
    }

    pub fn bottom_lines(&self, room: usize) -> Vec<Line<'static>> {
        let kept = self.kept_rows(room);
        let shown = &kept[kept.len().saturating_sub(room)..];
        let padding = room.saturating_sub(shown.len());
        let shown_lines = shown.iter().map(|&index| self.lines[index].clone());
        std::iter::repeat_n(Line::default(), padding)
            .chain(shown_lines)
            .collect()
    }

    fn kept_rows(&self, room: usize) -> Vec<usize> {
        let mut runs = Self::blank_runs(&self.lines);
        let mut excess = self.lines.len().saturating_sub(room);
        while excess > 0 {
            let shrinkable = runs.iter_mut().filter(|run| run.kept > 1);
            let Some(longest) = shrinkable.max_by_key(|run| run.kept) else {
                break;
            };
            longest.kept -= 1;
            excess -= 1;
        }
        (0..self.lines.len())
            .filter(|index| runs.iter().all(|run| run.keeps(*index)))
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

    fn styled_lines(raw: &str) -> Option<Vec<Line<'static>>> {
        raw.as_bytes().into_text().ok().map(|text| text.lines)
    }

    fn plain_lines(raw: &str) -> Vec<Line<'static>> {
        TerminalText::new(raw)
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
            PreviewText::parse(&screen)
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

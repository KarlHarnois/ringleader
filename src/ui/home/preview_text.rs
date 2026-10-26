use ansi_to_tui::IntoText;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

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

    pub fn wrapped_to(&self, width: usize) -> Self {
        let lines = self
            .lines
            .iter()
            .flat_map(|line| RowWrap::split(line, width))
            .collect();
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

struct RowWrap {
    width: usize,
    rows: Vec<Line<'static>>,
    row: Vec<Span<'static>>,
    row_width: usize,
}

impl RowWrap {
    const BOX_DRAWING: std::ops::RangeInclusive<char> = '\u{2500}'..='\u{257F}';

    fn split(line: &Line<'static>, width: usize) -> Vec<Line<'static>> {
        let mut wrap = Self {
            width,
            rows: Vec::new(),
            row: Vec::new(),
            row_width: 0,
        };
        for span in &line.spans {
            wrap.push_span(span);
        }
        wrap.finish(line)
    }

    fn push_span(&mut self, span: &Span<'static>) {
        for character in span.content.chars() {
            self.push_char(character, span.style);
        }
    }

    fn push_char(&mut self, character: char, style: Style) {
        let character_width = character.width().unwrap_or(0);
        let overflows = self.row_width + character_width > self.width;
        if overflows && self.row_width > 0 {
            self.break_row();
        }
        self.row_width += character_width;
        match self.row.last_mut() {
            Some(span) if span.style == style => span.content.to_mut().push(character),
            _ => self.row.push(Span::styled(character.to_string(), style)),
        }
    }

    fn break_row(&mut self) {
        self.rows.push(Line::from(std::mem::take(&mut self.row)));
        self.row_width = 0;
    }

    fn finish(mut self, line: &Line<'static>) -> Vec<Line<'static>> {
        self.break_row();
        let kept = if Self::is_rule(line) {
            1
        } else {
            self.rows.len() - self.trailing_blank_rows()
        };
        self.rows.truncate(kept.max(1));
        self.rows
            .into_iter()
            .map(|row| row.style(line.style))
            .collect()
    }

    fn trailing_blank_rows(&self) -> usize {
        self.rows
            .iter()
            .rev()
            .take_while(|row| PreviewText::is_blank(row))
            .count()
    }

    fn is_rule(line: &Line) -> bool {
        let mut visible = line
            .spans
            .iter()
            .flat_map(|span| span.content.chars())
            .filter(|character| !character.is_whitespace());
        visible.all(|character| Self::BOX_DRAWING.contains(&character))
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

        fn wrapped(raw: &str, width: usize) -> Vec<String> {
            let rows = PreviewText::parse(raw).wrapped_to(width).lines;
            rows.iter().map(Self::plain).collect()
        }

        fn plain(line: &Line) -> String {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
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

    #[test]
    fn a_row_wider_than_the_width_continues_on_the_next_row() {
        let wrapped = TextFixture::wrapped("I found the cause and fixed it.", 12);

        assert_eq!(wrapped, ["I found the ", "cause and fi", "xed it."]);
    }

    #[test]
    fn a_wide_character_that_would_straddle_the_edge_moves_to_the_next_row() {
        let wrapped = TextFixture::wrapped("abc漢字", 4);

        assert_eq!(wrapped, ["abc", "漢字"]);
    }

    #[test]
    fn wrapping_keeps_the_style_of_each_character() {
        let text = PreviewText::parse("\u{1b}[32mgreen\u{1b}[0mplain").wrapped_to(3);

        let second_row = &text.lines[1];
        let styles: Vec<_> = second_row.spans.iter().map(|span| span.style.fg).collect();
        assert_eq!(
            (TextFixture::plain(second_row), styles[0]),
            ("enp".to_string(), Some(ratatui::style::Color::Green))
        );
    }

    #[test]
    fn trailing_spaces_past_the_width_add_no_blank_rows() {
        let row = format!("prompt{}", " ".repeat(40));

        let wrapped = TextFixture::wrapped(&format!("{row}\nnext"), 10);

        assert_eq!(wrapped, ["prompt    ", "next"]);
    }

    #[test]
    fn a_horizontal_rule_wider_than_the_width_is_cut_to_one_row_because_it_holds_no_text() {
        let wrapped = TextFixture::wrapped(&"─".repeat(200), 10);

        assert_eq!(wrapped, ["─".repeat(10)]);
    }

    #[test]
    fn a_row_that_only_starts_with_a_rule_still_wraps() {
        let wrapped = TextFixture::wrapped("──── summary", 6);

        assert_eq!(wrapped, ["──── s", "ummary"]);
    }
}

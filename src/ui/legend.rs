use std::iter::{once, repeat};

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::theme::Theme;

pub struct Legend {
    label: String,
    entries: Vec<LegendEntry>,
}

impl Legend {
    const GAP: &'static str = "  ";

    pub fn new(label: String, entries: Vec<LegendEntry>) -> Self {
        Self { label, entries }
    }

    pub fn lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let label_width = self.label.chars().count();
        let room = width.saturating_sub(label_width);
        let labels = once(self.label.clone()).chain(repeat(" ".repeat(label_width)));
        labels
            .zip(self.rows(room))
            .map(|(label, row)| Self::line(label, &row, theme))
            .collect()
    }

    fn rows(&self, room: usize) -> Vec<Vec<&LegendEntry>> {
        let mut rows = Vec::new();
        let mut row = Vec::new();
        for entry in &self.entries {
            if Self::overflows(&row, entry, room) {
                rows.push(std::mem::take(&mut row));
            }
            row.push(entry);
        }
        rows.push(row);
        rows
    }

    fn overflows(row: &[&LegendEntry], entry: &LegendEntry, room: usize) -> bool {
        let widened = Self::width(row) + Self::GAP.len() + entry.width();
        !row.is_empty() && widened > room
    }

    fn width(row: &[&LegendEntry]) -> usize {
        let gaps = Self::GAP.len() * row.len().saturating_sub(1);
        row.iter().map(|entry| entry.width()).sum::<usize>() + gaps
    }

    fn line(label: String, row: &[&LegendEntry], theme: &Theme) -> Line<'static> {
        let label = Span::styled(label, theme.base());
        let entries: Vec<Vec<Span<'static>>> = row.iter().map(|entry| entry.spans(theme)).collect();
        Line::from([vec![label], entries.join(&Span::raw(Self::GAP))].concat())
    }
}

pub struct LegendEntry {
    symbol: String,
    style: Style,
    meaning: &'static str,
}

impl LegendEntry {
    pub fn new(symbol: impl ToString, style: Style, meaning: &'static str) -> Self {
        Self {
            symbol: symbol.to_string(),
            style,
            meaning,
        }
    }

    fn spans(&self, theme: &Theme) -> Vec<Span<'static>> {
        vec![
            Span::styled(self.symbol.clone(), self.style),
            Span::styled(format!(" {}", self.meaning), theme.base()),
        ]
    }

    fn width(&self) -> usize {
        self.symbol.chars().count() + 1 + self.meaning.chars().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LegendFixture;

    impl LegendFixture {
        const LABEL: &'static str = "Fruit  ";

        fn legend() -> Legend {
            let entries = [("A", "apple"), ("B", "banana"), ("C", "cherry")]
                .map(|(symbol, meaning)| LegendEntry::new(symbol, Theme.base(), meaning));
            Legend::new(Self::LABEL.to_string(), entries.into())
        }

        fn rows(width: usize) -> Vec<String> {
            Self::legend()
                .lines(width, &Theme)
                .iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<String>()
                })
                .collect()
        }
    }

    #[test]
    fn a_wide_legend_keeps_every_entry_on_the_label_row() {
        assert_eq!(
            LegendFixture::rows(80),
            ["Fruit  A apple  B banana  C cherry"]
        );
    }

    #[test]
    fn a_narrow_legend_wraps_entries_under_the_label() {
        assert_eq!(
            LegendFixture::rows(25),
            ["Fruit  A apple  B banana", "       C cherry"]
        );
    }

    #[test]
    fn an_entry_wider_than_the_room_gets_a_row_of_its_own() {
        assert_eq!(
            LegendFixture::rows(10),
            ["Fruit  A apple", "       B banana", "       C cherry"]
        );
    }

    #[test]
    fn each_symbol_keeps_its_own_style() {
        let entry = LegendEntry::new("+", Theme.insertions(), "added");
        let legend = Legend::new(LegendFixture::LABEL.to_string(), vec![entry]);

        let line = &legend.lines(80, &Theme)[0];

        assert_eq!(line.spans[1].style, Theme.insertions());
    }
}

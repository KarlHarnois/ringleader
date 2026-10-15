use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::label_column::LabelColumn;
use crate::ui::theme::Theme;

pub struct Legend {
    column: LabelColumn,
    entries: Vec<LegendEntry>,
}

impl Legend {
    const GAP: &'static str = "  ";

    pub fn new(label: &str, entries: Vec<LegendEntry>) -> Self {
        Self {
            column: LabelColumn::new(label),
            entries,
        }
    }

    pub fn lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let rows = self.rows(self.column.room(width));
        let spans = rows.iter().map(|row| Self::spans(row, theme));
        self.column.lines(spans, theme)
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
        let widened = Self::width(row) + Self::GAP.chars().count() + entry.width();
        !row.is_empty() && widened > room
    }

    fn width(row: &[&LegendEntry]) -> usize {
        let gaps = Self::GAP.chars().count() * row.len().saturating_sub(1);
        row.iter().map(|entry| entry.width()).sum::<usize>() + gaps
    }

    fn spans(row: &[&LegendEntry], theme: &Theme) -> Vec<Span<'static>> {
        let entries: Vec<Vec<Span<'static>>> = row.iter().map(|entry| entry.spans(theme)).collect();
        entries.join(&Span::raw(Self::GAP))
    }
}

pub struct LegendEntry {
    symbol: String,
    style: Style,
    meaning: &'static str,
}

impl LegendEntry {
    const SYMBOL_GAP: &'static str = " ";

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
            Span::styled(
                format!("{}{}", Self::SYMBOL_GAP, self.meaning),
                theme.base(),
            ),
        ]
    }

    fn width(&self) -> usize {
        self.symbol.chars().count()
            + Self::SYMBOL_GAP.chars().count()
            + self.meaning.chars().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LegendFixture;

    impl LegendFixture {
        const LABEL: &'static str = "Fruit";

        fn legend() -> Legend {
            let entries = [("A", "apple"), ("B", "banana"), ("C", "cherry")]
                .map(|(symbol, meaning)| LegendEntry::new(symbol, Theme.base(), meaning));
            Legend::new(Self::LABEL, entries.into())
        }

        fn rows(width: usize) -> Vec<String> {
            Self::legend()
                .lines(width, &Theme)
                .iter()
                .map(ToString::to_string)
                .collect()
        }
    }

    #[test]
    fn a_wide_legend_keeps_every_entry_on_the_label_row() {
        assert_eq!(
            LegendFixture::rows(80),
            ["Fruit        A apple  B banana  C cherry"]
        );
    }

    #[test]
    fn a_row_that_fills_its_room_exactly_stays_on_one_line() {
        assert_eq!(
            LegendFixture::rows(30),
            ["Fruit        A apple  B banana", "             C cherry"]
        );
    }

    #[test]
    fn a_row_one_column_past_its_room_wraps_under_the_label() {
        assert_eq!(
            LegendFixture::rows(29),
            [
                "Fruit        A apple",
                "             B banana",
                "             C cherry"
            ]
        );
    }

    #[test]
    fn an_entry_wider_than_the_room_gets_a_row_of_its_own() {
        assert_eq!(
            LegendFixture::rows(10),
            [
                "Fruit        A apple",
                "             B banana",
                "             C cherry"
            ]
        );
    }

    #[test]
    fn each_symbol_keeps_its_own_style() {
        let entry = LegendEntry::new("+", Theme.insertions(), "added");
        let legend = Legend::new(LegendFixture::LABEL, vec![entry]);

        let line = &legend.lines(80, &Theme)[0];

        assert_eq!(line.spans[1].style, Theme.insertions());
    }
}

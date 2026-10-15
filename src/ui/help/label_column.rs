use std::iter::{once, repeat};

use ratatui::text::{Line, Span};

use crate::ui::theme::Theme;

pub struct LabelColumn {
    label: String,
}

impl LabelColumn {
    const WIDTH: usize = 11;
    const GAP: &'static str = "  ";

    pub fn new(label: &str) -> Self {
        Self {
            label: format!("{label:<width$}{}", Self::GAP, width = Self::WIDTH),
        }
    }

    pub fn room(&self, width: usize) -> usize {
        width.saturating_sub(self.width())
    }

    pub fn lines(
        &self,
        rows: impl IntoIterator<Item = Vec<Span<'static>>>,
        theme: &Theme,
    ) -> Vec<Line<'static>> {
        let indent = " ".repeat(self.width());
        let prefixes = once(self.label.clone()).chain(repeat(indent));
        prefixes
            .zip(rows)
            .map(|(prefix, row)| {
                let prefix = Span::styled(prefix, theme.base());
                Line::from([vec![prefix], row].concat())
            })
            .collect()
    }

    fn width(&self) -> usize {
        self.label.chars().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ColumnFixture;

    impl ColumnFixture {
        fn texts(column: &LabelColumn, rows: &[&str]) -> Vec<String> {
            let rows = rows.iter().map(|row| vec![Span::raw(row.to_string())]);
            column
                .lines(rows, &Theme)
                .iter()
                .map(ToString::to_string)
                .collect()
        }
    }

    #[test]
    fn a_short_label_pads_to_the_column() {
        let column = LabelColumn::new("Log");

        assert_eq!(
            ColumnFixture::texts(&column, &["app.log"]),
            ["Log          app.log"]
        );
    }

    #[test]
    fn a_long_label_shows_in_full_with_a_gap() {
        let column = LabelColumn::new("Ctrl-Alt-Shift-F12");

        assert_eq!(
            ColumnFixture::texts(&column, &["return"]),
            ["Ctrl-Alt-Shift-F12  return"]
        );
    }

    #[test]
    fn later_rows_indent_under_the_first() {
        let column = LabelColumn::new("Log");

        assert_eq!(
            ColumnFixture::texts(&column, &["first", "second"]),
            ["Log          first", "             second"]
        );
    }

    #[test]
    fn the_room_is_the_width_left_after_the_label() {
        assert_eq!(LabelColumn::new("Log").room(40), 27);
        assert_eq!(LabelColumn::new("Ctrl-Alt-Shift-F12").room(40), 20);
    }
}

use ratatui::style::Style;
use ratatui::text::Span;

pub struct ColoredRow<'t> {
    spans: Vec<Span<'static>>,
    remaining: &'t str,
    base: Style,
}

impl<'t> ColoredRow<'t> {
    pub fn new(text: &'t str, base: Style) -> Self {
        Self {
            spans: Vec::new(),
            remaining: text,
            base,
        }
    }

    pub fn skip(self, cell: &str) -> Self {
        let unstyled = vec![Span::styled(cell.to_string(), self.base)];
        self.color_first(cell, unstyled)
    }

    pub fn color_first(self, cell: &str, styled: Vec<Span<'static>>) -> Self {
        let start = self.remaining.find(cell);
        self.color_at(start, cell, styled)
    }

    pub fn color_last(self, cell: &str, styled: Vec<Span<'static>>) -> Self {
        let start = self.remaining.rfind(cell);
        self.color_at(start, cell, styled)
    }

    pub fn into_spans(mut self) -> Vec<Span<'static>> {
        if !self.remaining.is_empty() {
            self.spans
                .push(Span::styled(self.remaining.to_string(), self.base));
        }
        self.spans
    }

    fn color_at(mut self, start: Option<usize>, cell: &str, styled: Vec<Span<'static>>) -> Self {
        let Some(start) = start.filter(|_| !cell.is_empty()) else {
            return self;
        };
        let (before, rest) = self.remaining.split_at(start);
        self.spans.push(Span::styled(before.to_string(), self.base));
        self.spans.extend(styled);
        self.remaining = &rest[cell.len()..];
        self
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::*;

    struct RowFixture;

    impl RowFixture {
        fn base() -> Style {
            Style::default()
        }

        fn red(text: &str) -> Vec<Span<'static>> {
            vec![Span::styled(
                text.to_string(),
                Style::default().fg(Color::Red),
            )]
        }

        fn texts(spans: &[Span<'static>]) -> Vec<String> {
            spans.iter().map(|span| span.content.to_string()).collect()
        }
    }

    #[test]
    fn a_row_without_cells_is_one_base_span() {
        let spans = ColoredRow::new("fix-auth main", RowFixture::base()).into_spans();

        assert_eq!(RowFixture::texts(&spans), ["fix-auth main"]);
    }

    #[test]
    fn a_cell_splits_the_row_around_its_styled_spans() {
        let spans = ColoredRow::new("fix-auth #142 main", RowFixture::base())
            .color_first("#142", RowFixture::red("#142"))
            .into_spans();

        assert_eq!(RowFixture::texts(&spans), ["fix-auth ", "#142", " main"]);
        assert_eq!(spans[1].style.fg, Some(Color::Red));
    }

    #[test]
    fn a_skipped_cell_hides_its_text_from_the_next_cell() {
        let spans = ColoredRow::new("claude-docs claude", RowFixture::base())
            .skip("claude-docs")
            .color_first("claude", RowFixture::red("claude"))
            .into_spans();

        let last = spans.last().unwrap();
        assert_eq!(RowFixture::texts(&spans).concat(), "claude-docs claude");
        assert_eq!(
            (last.content.as_ref(), last.style.fg),
            ("claude", Some(Color::Red))
        );
    }

    #[test]
    fn the_last_cell_is_found_from_the_end() {
        let spans = ColoredRow::new("+1 -0 and +1 -0", RowFixture::base())
            .color_last("+1 -0", RowFixture::red("+1 -0"))
            .into_spans();

        assert_eq!(RowFixture::texts(&spans), ["+1 -0 and ", "+1 -0"]);
    }

    #[test]
    fn a_cell_missing_from_the_row_leaves_it_whole() {
        let spans = ColoredRow::new("fix-auth main", RowFixture::base())
            .color_first("#142", RowFixture::red("#142"))
            .into_spans();

        assert_eq!(RowFixture::texts(&spans), ["fix-auth main"]);
    }
}

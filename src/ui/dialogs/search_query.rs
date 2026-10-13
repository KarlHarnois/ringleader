use crossterm::event::KeyEvent;
use ratatui::text::Line;

use super::text_field::{FieldKind, TextField};

pub struct SearchQuery {
    field: TextField,
}

impl SearchQuery {
    const LABEL: &'static str = "Find: ";
    const CURSOR: char = '█';

    pub fn new(kind: FieldKind) -> Self {
        Self {
            field: TextField::new(kind),
        }
    }

    pub fn value(&self) -> &str {
        self.field.value()
    }

    pub fn edit(&mut self, key: &KeyEvent) -> bool {
        self.field.edit(key)
    }

    pub fn paste(&mut self, text: &str) -> Result<(), String> {
        self.field.paste(text)
    }

    pub fn line(&self, width: usize) -> Line<'static> {
        let room = width.saturating_sub(Self::LABEL.len());
        let shown_query = self.field.visible(room, Some(Self::CURSOR));
        Line::from(format!("{}{shown_query}", Self::LABEL))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_query_shows_the_label_and_the_cursor() {
        let query = SearchQuery::new(FieldKind::Name);

        assert_eq!(query.line(40).to_string(), "Find: █");
    }

    #[test]
    fn a_long_query_shows_its_end_after_the_label() {
        let mut query = SearchQuery::new(FieldKind::Name);
        query.paste("refactor-the-reconciler").unwrap();

        assert_eq!(query.line(14).to_string(), "Find: onciler█");
    }
}

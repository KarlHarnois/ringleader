use crossterm::event::KeyEvent;

use super::text_field::{FieldKind, TextField};

pub struct SearchQuery {
    field: TextField,
    prompt: &'static str,
}

impl SearchQuery {
    const CURSOR: char = '█';

    pub fn new(kind: FieldKind, prompt: &'static str) -> Self {
        Self {
            field: TextField::new(kind),
            prompt,
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

    pub fn shown(&self, width: usize) -> String {
        let room = width.saturating_sub(self.prompt.chars().count());
        let shown_query = self.field.visible(room, Some(Self::CURSOR));
        format!("{}{shown_query}", self.prompt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct QueryFixture;

    impl QueryFixture {
        const PROMPT: &'static str = "❯ ";
    }

    #[test]
    fn an_empty_query_shows_the_prompt_and_the_cursor() {
        let query = SearchQuery::new(FieldKind::Name, QueryFixture::PROMPT);

        assert_eq!(query.shown(40), "❯ █");
    }

    #[test]
    fn a_long_query_shows_its_end_after_the_prompt() {
        let mut query = SearchQuery::new(FieldKind::Name, QueryFixture::PROMPT);
        query.paste("refactor-the-reconciler").unwrap();

        assert_eq!(query.shown(10), "❯ onciler█");
    }
}

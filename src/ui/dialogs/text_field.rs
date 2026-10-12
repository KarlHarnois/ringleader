use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::text_fit::TextFit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Name,
    Prompt,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextField {
    kind: FieldKind,
    value: String,
}

impl TextField {
    const LINE_BREAK_MODIFIER: KeyModifiers = KeyModifiers::ALT;
    const LINE_BREAK_IN_PATH: &'static str = "a path cannot contain a line break";

    pub fn new(kind: FieldKind) -> Self {
        Self {
            kind,
            value: String::new(),
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn visible(&self, width: usize, cursor: Option<char>) -> String {
        let shown = self.with_cursor(cursor);
        let skipped = shown.chars().count().saturating_sub(width);
        shown.chars().skip(skipped).collect()
    }

    pub fn visible_rows(&self, width: usize, max_rows: usize, cursor: Option<char>) -> Vec<String> {
        let rows = self.wrapped(width, cursor);
        let skipped = rows.len().saturating_sub(max_rows);
        rows.into_iter().skip(skipped).collect()
    }

    pub fn edit(&mut self, key: &KeyEvent) -> bool {
        let typed_text = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        match key.code {
            KeyCode::Enter if self.accepts_line_break(key) => {
                self.value.push('\n');
                true
            }
            KeyCode::Char(character) if typed_text && !character.is_control() => {
                self.value.push(character);
                true
            }
            KeyCode::Char('w') if key.modifiers == KeyModifiers::CONTROL => {
                self.delete_word();
                true
            }
            KeyCode::Backspace => {
                self.value.pop();
                true
            }
            _ => false,
        }
    }

    pub fn accepts_line_break(&self, key: &KeyEvent) -> bool {
        self.kind == FieldKind::Prompt && key.modifiers == Self::LINE_BREAK_MODIFIER
    }

    pub fn paste(&mut self, text: &str) -> Result<(), String> {
        let lines: Vec<&str> = text
            .split("\r\n")
            .flat_map(|part| part.split(['\r', '\n']))
            .collect();
        let pasted = match self.kind {
            FieldKind::Name => lines.join(" "),
            FieldKind::Prompt => lines.join("\n"),
            FieldKind::Path => Self::single_line_path(text)?,
        };
        self.value.push_str(&pasted);
        Ok(())
    }

    fn wrapped(&self, width: usize, cursor: Option<char>) -> Vec<String> {
        self.with_cursor(cursor)
            .split('\n')
            .flat_map(|line| TextFit::new(line).wrapped(width))
            .collect()
    }

    fn with_cursor(&self, cursor: Option<char>) -> String {
        self.value.chars().chain(cursor).collect()
    }

    fn delete_word(&mut self) {
        let kept = self
            .value
            .trim_end()
            .trim_end_matches(|character: char| !character.is_whitespace());
        self.value.truncate(kept.len());
    }

    fn single_line_path(text: &str) -> Result<String, String> {
        let trimmed = text.trim();
        if trimmed.contains(['\r', '\n']) {
            return Err(Self::LINE_BREAK_IN_PATH.to_string());
        }
        Ok(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FieldFixture;

    impl FieldFixture {
        fn key(code: KeyCode) -> KeyEvent {
            KeyEvent::new(code, KeyModifiers::NONE)
        }

        fn alt_enter() -> KeyEvent {
            KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)
        }

        fn after_control_w(kind: FieldKind, text: &str) -> (bool, String) {
            let mut field = Self::holding(kind, text);
            let consumed = field.edit(&KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
            (consumed, field.value().to_string())
        }

        fn holding(kind: FieldKind, text: &str) -> TextField {
            let mut field = TextField::new(kind);
            field.paste(text).unwrap();
            field
        }

        fn pasted(kind: FieldKind, text: &str) -> Result<String, String> {
            let mut field = TextField::new(kind);
            field.paste(text)?;
            Ok(field.value().to_string())
        }
    }

    #[test]
    fn typed_characters_append_and_backspace_removes_the_last() {
        let mut field = TextField::new(FieldKind::Name);

        for character in "fix-authx".chars() {
            field.edit(&FieldFixture::key(KeyCode::Char(character)));
        }
        field.edit(&FieldFixture::key(KeyCode::Backspace));

        assert_eq!(field.value(), "fix-auth");
    }

    #[test]
    fn control_w_deletes_the_last_word_and_keeps_the_space_before_it() {
        let (_, value) = FieldFixture::after_control_w(FieldKind::Name, "fix the tests");

        assert_eq!(value, "fix the ");
    }

    #[test]
    fn control_w_takes_trailing_spaces_with_the_word_before_them() {
        let (_, value) = FieldFixture::after_control_w(FieldKind::Name, "fix the  ");

        assert_eq!(value, "fix ");
    }

    #[test]
    fn control_w_deletes_a_path_without_spaces_whole() {
        let (_, value) = FieldFixture::after_control_w(FieldKind::Path, "~/code/example");

        assert_eq!(value, "");
    }

    #[test]
    fn control_w_in_a_prompt_stops_at_the_line_break() {
        let (_, value) = FieldFixture::after_control_w(FieldKind::Prompt, "first line\nsecond");

        assert_eq!(value, "first line\n");
    }

    #[test]
    fn control_w_on_an_empty_field_is_handled_and_changes_nothing() {
        let (consumed, value) = FieldFixture::after_control_w(FieldKind::Name, "");

        assert!(consumed);
        assert_eq!(value, "");
    }

    #[test]
    fn a_control_chord_is_not_typed_text() {
        let mut field = TextField::new(FieldKind::Name);

        let consumed = field.edit(&KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));

        assert!(!consumed);
        assert_eq!(field.value(), "");
    }

    #[test]
    fn a_name_field_turns_each_pasted_line_break_into_a_space() {
        assert_eq!(
            FieldFixture::pasted(FieldKind::Name, "fix\r\nthe\rlogin\nflow"),
            Ok("fix the login flow".to_string())
        );
    }

    #[test]
    fn a_prompt_field_keeps_every_kind_of_line_break_as_a_newline() {
        assert_eq!(
            FieldFixture::pasted(FieldKind::Prompt, "one\r\ntwo\rthree\nfour"),
            Ok("one\ntwo\nthree\nfour".to_string())
        );
    }

    #[test]
    fn a_prompt_wraps_the_word_that_reaches_the_edge_onto_the_next_row() {
        let field = FieldFixture::holding(FieldKind::Prompt, "fix the login flow");

        assert_eq!(field.visible_rows(9, 4, None), ["fix the", "login", "flow"]);
    }

    #[test]
    fn a_word_longer_than_the_row_breaks_inside_the_word() {
        let field = FieldFixture::holding(FieldKind::Prompt, "refactor-the-reconciler");

        assert_eq!(
            field.visible_rows(8, 4, None),
            ["refactor", "-the-rec", "onciler"]
        );
    }

    #[test]
    fn a_line_break_starts_a_new_row_and_the_cursor_follows_the_text() {
        let field = FieldFixture::holding(FieldKind::Prompt, "one\ntwo");

        assert_eq!(field.visible_rows(10, 4, Some('█')), ["one", "two█"]);
    }

    #[test]
    fn a_long_prompt_shows_its_last_rows() {
        let field = FieldFixture::holding(FieldKind::Prompt, "one\ntwo\nthree");

        assert_eq!(field.visible_rows(10, 2, None), ["two", "three"]);
    }

    #[test]
    fn alt_enter_adds_a_line_break_to_a_prompt() {
        let mut field = FieldFixture::holding(FieldKind::Prompt, "one");

        field.edit(&FieldFixture::alt_enter());

        assert_eq!(field.value(), "one\n");
        assert_eq!(field.visible_rows(10, 4, Some('█')), ["one", "█"]);
    }

    #[test]
    fn alt_enter_is_ignored_by_a_name_field() {
        let mut field = FieldFixture::holding(FieldKind::Name, "fix-auth");

        let consumed = field.edit(&FieldFixture::alt_enter());

        assert!(!consumed);
        assert_eq!(field.value(), "fix-auth");
    }

    #[test]
    fn a_long_value_shows_its_end_with_the_cursor() {
        let field = FieldFixture::holding(FieldKind::Name, "refactor-the-reconciler");

        assert_eq!(field.visible(8, Some('█')), "onciler█");
    }

    #[test]
    fn a_path_field_trims_whitespace_and_line_breaks_around_a_paste() {
        assert_eq!(
            FieldFixture::pasted(FieldKind::Path, "  ~/code/example-project\n"),
            Ok("~/code/example-project".to_string())
        );
    }

    #[test]
    fn a_line_break_inside_a_pasted_path_is_a_field_error() {
        assert_eq!(
            FieldFixture::pasted(FieldKind::Path, "~/code\n/example-project"),
            Err("a path cannot contain a line break".to_string())
        );
    }
}

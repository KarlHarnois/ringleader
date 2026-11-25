use crate::domain::Prompt;

pub struct PromptText {
    text: String,
}

impl PromptText {
    pub fn new(text: String) -> Self {
        Self { text }
    }

    pub fn parse(text: String) -> Option<Self> {
        let text = Self::without_trailing_line_ends(&text);
        (!Prompt::of(text).is_blank()).then(|| Self {
            text: text.to_string(),
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    fn without_trailing_line_ends(text: &str) -> &str {
        let mut end = text.len();
        while let Some(without_newline) = text[..end].strip_suffix('\n') {
            end = without_newline.len();
            if let Some(without_carriage_return) = text[..end].strip_suffix('\r') {
                end = without_carriage_return.len();
            }
        }
        &text[..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_that_holds_a_prompt_keeps_it() {
        let prompt = PromptText::parse("fix the login".to_string());

        assert_eq!(prompt.as_ref().map(PromptText::text), Some("fix the login"));
    }

    #[test]
    fn trailing_line_ends_are_dropped() {
        let prompt = PromptText::parse("fix the login\n\r\n".to_string());

        assert_eq!(prompt.as_ref().map(PromptText::text), Some("fix the login"));
    }

    #[test]
    fn leading_spaces_are_kept() {
        let prompt = PromptText::parse("  fix the login\n".to_string());

        assert_eq!(
            prompt.as_ref().map(PromptText::text),
            Some("  fix the login")
        );
    }

    #[test]
    fn a_text_of_only_line_ends_holds_no_prompt() {
        assert!(PromptText::parse("\n\r\n\n".to_string()).is_none());
    }

    #[test]
    fn a_text_of_only_spaces_holds_no_prompt() {
        assert!(PromptText::parse("  \n".to_string()).is_none());
    }

    #[test]
    fn a_given_text_is_kept_unchanged() {
        let prompt = PromptText::new("  fix the login\n".to_string());

        assert_eq!(prompt.text(), "  fix the login\n");
    }
}

pub struct Prompt<'a> {
    text: &'a str,
}

impl<'a> Prompt<'a> {
    pub fn of(text: &'a str) -> Self {
        Self { text }
    }

    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_prompt_is_blank() {
        assert!(Prompt::of("").is_blank());
    }

    #[test]
    fn a_prompt_of_spaces_is_blank() {
        assert!(Prompt::of("   ").is_blank());
    }

    #[test]
    fn a_prompt_of_whitespace_characters_is_blank() {
        assert!(Prompt::of(" \n\t").is_blank());
    }

    #[test]
    fn a_prompt_with_text_around_whitespace_is_not_blank() {
        assert!(!Prompt::of(" fix the login ").is_blank());
    }
}

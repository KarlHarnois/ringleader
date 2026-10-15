use std::fmt;

pub struct ShellWord<'a> {
    word: &'a str,
}

impl<'a> ShellWord<'a> {
    const SAFE_PUNCTUATION: &'static str = "/._-:@%+=,";

    pub fn new(word: &'a str) -> Self {
        Self { word }
    }

    fn needs_no_quotes(&self) -> bool {
        !self.word.is_empty()
            && self.word.chars().all(|character| {
                character.is_ascii_alphanumeric() || Self::SAFE_PUNCTUATION.contains(character)
            })
    }
}

impl fmt::Display for ShellWord<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.needs_no_quotes() {
            true => formatter.write_str(self.word),
            false => write!(formatter, "'{}'", self.word.replace('\'', r"'\''")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_of_safe_characters_stays_bare() {
        assert_eq!(
            ShellWord::new("/repos/example-project_v1.2").to_string(),
            "/repos/example-project_v1.2"
        );
    }

    #[test]
    fn a_word_with_a_space_is_quoted() {
        assert_eq!(
            ShellWord::new("/home/Jane Doe").to_string(),
            "'/home/Jane Doe'"
        );
    }

    #[test]
    fn a_single_quote_is_escaped_inside_the_quotes() {
        assert_eq!(ShellWord::new("it's").to_string(), r"'it'\''s'");
    }

    #[test]
    fn an_empty_word_is_quoted() {
        assert_eq!(ShellWord::new("").to_string(), "''");
    }
}

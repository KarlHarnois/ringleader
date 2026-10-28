pub struct ParseFailure<'a> {
    text: &'a str,
    error: &'a toml::de::Error,
}

impl<'a> ParseFailure<'a> {
    pub fn new(text: &'a str, error: &'a toml::de::Error) -> Self {
        Self { text, error }
    }

    pub fn describe(&self) -> String {
        let message = self.error.message().trim();
        match self.offending_line() {
            Some(line) => format!("`{}`: {message}", line.trim()),
            None => message.to_string(),
        }
    }

    fn offending_line(&self) -> Option<&'a str> {
        let start = self.error.span()?.start.min(self.text.len());
        let line_start = self.text[..start].rfind('\n').map_or(0, |index| index + 1);
        self.text[line_start..].lines().next()
    }
}

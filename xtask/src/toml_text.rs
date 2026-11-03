pub struct TomlText {
    text: String,
}

impl TomlText {
    const TABLE_START: char = '[';
    const ASSIGNMENT: &'static str = " = ";
    const QUOTE: char = '"';

    pub fn new(text: String) -> Self {
        Self { text }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn string_value(&self, anchor: &str, key: &str) -> Option<&str> {
        let index = self.entry_index(anchor, key)?;
        self.text
            .lines()
            .nth(index)
            .and_then(Self::entry)
            .map(|(_, value)| value)
    }

    pub fn with_string_value(&self, anchor: &str, key: &str, value: &str) -> Option<Self> {
        let index = self.entry_index(anchor, key)?;
        let replacement = format!(
            "{key}{}{quote}{value}{quote}\n",
            Self::ASSIGNMENT,
            quote = Self::QUOTE
        );
        let text = self
            .text
            .split_inclusive('\n')
            .enumerate()
            .map(|(position, line)| {
                if position == index {
                    &replacement
                } else {
                    line
                }
            })
            .collect();
        Some(Self::new(text))
    }

    fn entry_index(&self, anchor: &str, key: &str) -> Option<usize> {
        let lines: Vec<&str> = self.text.lines().collect();
        let start = lines.iter().position(|line| *line == anchor)?;
        lines[start..]
            .iter()
            .enumerate()
            .take_while(|(offset, line)| *offset == 0 || !line.starts_with(Self::TABLE_START))
            .find(|(_, line)| Self::entry(line).is_some_and(|(name, _)| name == key))
            .map(|(offset, _)| start + offset)
    }

    fn entry(line: &str) -> Option<(&str, &str)> {
        let (name, value) = line.split_once(Self::ASSIGNMENT)?;
        let unquoted = value.strip_prefix(Self::QUOTE)?.strip_suffix(Self::QUOTE)?;
        Some((name, unquoted))
    }
}

#[cfg(test)]
mod tests {
    use super::TomlText;

    struct TomlFixture;

    impl TomlFixture {
        const TEXT: &'static str =
            "[package]\nname = \"example-project\"\n\n[dependencies]\nversion = \"9.9.9\"\n";

        fn text() -> TomlText {
            TomlText::new(Self::TEXT.to_string())
        }
    }

    #[test]
    fn a_value_is_read_from_the_anchored_table() {
        assert_eq!(
            TomlFixture::text().string_value("[package]", "name"),
            Some("example-project")
        );
    }

    #[test]
    fn a_key_of_a_later_table_is_not_found() {
        assert_eq!(
            TomlFixture::text().string_value("[package]", "version"),
            None
        );
    }

    #[test]
    fn a_missing_anchor_finds_nothing() {
        assert!(
            TomlFixture::text()
                .with_string_value("[workspace]", "name", "other")
                .is_none()
        );
    }
}

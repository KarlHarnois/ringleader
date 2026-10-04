pub struct ToolVersion<'a> {
    text: &'a str,
}

impl<'a> ToolVersion<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { text }
    }

    pub fn numbers(&self) -> Option<(u32, u32)> {
        let mut parts = self.text.split('.');
        let major = Self::leading_number(parts.next()?)?;
        let minor = Self::leading_number(parts.next()?)?;
        Some((major, minor))
    }

    fn leading_number(part: &str) -> Option<u32> {
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    }
}

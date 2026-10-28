#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeColor {
    Default,
    Ansi(AnsiColor),
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiColor {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

impl ThemeColor {
    const DEFAULT_NAME: &'static str = "default";
    const HEX_PREFIX: char = '#';
    const HEX_DIGITS: usize = 6;
    const CHANNEL_DIGITS: usize = 2;
    const HEX_RADIX: u32 = 16;

    pub fn parse(text: &str) -> Result<Self, String> {
        Self::parsed(text).ok_or_else(|| {
            format!("`{text}` is not a color: use `default`, an ANSI name, 0-255, or #rrggbb")
        })
    }

    fn parsed(text: &str) -> Option<Self> {
        if text == Self::DEFAULT_NAME {
            return Some(Self::Default);
        }
        AnsiColor::named(text)
            .map(Self::Ansi)
            .or_else(|| text.parse().ok().map(Self::Indexed))
            .or_else(|| Self::hex(text))
    }

    fn hex(text: &str) -> Option<Self> {
        let digits = text
            .strip_prefix(Self::HEX_PREFIX)
            .filter(|digits| digits.len() == Self::HEX_DIGITS)
            .filter(|digits| digits.chars().all(|digit| digit.is_ascii_hexdigit()))?;
        Some(Self::Rgb(
            Self::channel(digits, 0)?,
            Self::channel(digits, 1)?,
            Self::channel(digits, 2)?,
        ))
    }

    fn channel(digits: &str, ordinal: usize) -> Option<u8> {
        let start = ordinal * Self::CHANNEL_DIGITS;
        u8::from_str_radix(
            &digits[start..start + Self::CHANNEL_DIGITS],
            Self::HEX_RADIX,
        )
        .ok()
    }
}

impl AnsiColor {
    const NAMES: [(&'static str, AnsiColor); 16] = [
        ("black", AnsiColor::Black),
        ("red", AnsiColor::Red),
        ("green", AnsiColor::Green),
        ("yellow", AnsiColor::Yellow),
        ("blue", AnsiColor::Blue),
        ("magenta", AnsiColor::Magenta),
        ("cyan", AnsiColor::Cyan),
        ("white", AnsiColor::White),
        ("bright_black", AnsiColor::BrightBlack),
        ("bright_red", AnsiColor::BrightRed),
        ("bright_green", AnsiColor::BrightGreen),
        ("bright_yellow", AnsiColor::BrightYellow),
        ("bright_blue", AnsiColor::BrightBlue),
        ("bright_magenta", AnsiColor::BrightMagenta),
        ("bright_cyan", AnsiColor::BrightCyan),
        ("bright_white", AnsiColor::BrightWhite),
    ];

    fn named(text: &str) -> Option<Self> {
        Self::NAMES
            .iter()
            .find(|(name, _)| *name == text)
            .map(|(_, color)| *color)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_keeps_the_terminal_color() {
        assert_eq!(ThemeColor::parse("default"), Ok(ThemeColor::Default));
    }

    #[test]
    fn an_ansi_name_follows_the_terminal_palette() {
        assert_eq!(
            ThemeColor::parse("bright_cyan"),
            Ok(ThemeColor::Ansi(AnsiColor::BrightCyan))
        );
    }

    #[test]
    fn a_number_picks_a_256_color_index() {
        assert_eq!(ThemeColor::parse("238"), Ok(ThemeColor::Indexed(238)));
    }

    #[test]
    fn a_hex_value_fixes_an_exact_color_in_either_case() {
        assert_eq!(
            ThemeColor::parse("#8A8a8d"),
            Ok(ThemeColor::Rgb(0x8a, 0x8a, 0x8d))
        );
    }

    #[test]
    fn an_index_past_255_is_refused() {
        assert!(ThemeColor::parse("256").is_err());
    }

    #[test]
    fn a_short_hex_value_is_refused() {
        assert!(ThemeColor::parse("#fff").is_err());
    }

    #[test]
    fn a_refusal_names_the_bad_value() {
        let refusal = ThemeColor::parse("teal").unwrap_err();

        assert!(refusal.contains("`teal`"), "{refusal}");
    }
}

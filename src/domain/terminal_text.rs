pub struct TerminalText<'a> {
    raw: &'a str,
}

impl<'a> TerminalText<'a> {
    const ESCAPE: char = '\u{1b}';
    const BELL: char = '\u{7}';
    const CONTROL_SEQUENCE: char = '[';
    const OPERATING_SYSTEM_COMMAND: char = ']';
    const STRING_TERMINATOR: char = '\\';

    pub fn new(raw: &'a str) -> Self {
        Self { raw }
    }

    pub fn plain(&self) -> String {
        let mut plain = String::with_capacity(self.raw.len());
        let mut characters = self.raw.chars();
        while let Some(character) = characters.next() {
            if character != Self::ESCAPE {
                plain.push(character);
                continue;
            }
            match characters.next() {
                Some(Self::CONTROL_SEQUENCE) => {
                    Self::skip_through(&mut characters, |next| ('@'..='~').contains(&next))
                }
                Some(Self::OPERATING_SYSTEM_COMMAND) => {
                    Self::skip_through(&mut characters, |next| {
                        next == Self::BELL || next == Self::STRING_TERMINATOR
                    })
                }
                _ => {}
            }
        }
        plain
    }

    fn skip_through(characters: &mut impl Iterator<Item = char>, ends: impl Fn(char) -> bool) {
        for next in characters.by_ref() {
            if ends(next) {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plain_text_drops_every_escape_sequence() {
        let raw =
            "\u{1b}[1;32mready\u{1b}[0m \u{1b}]8;;https://example.com\u{7}link\u{1b}]8;;\u{1b}\\";

        assert_eq!(TerminalText::new(raw).plain(), "ready link");
    }

    #[test]
    fn text_without_escape_sequences_stays_as_it_is() {
        let raw = "direnv: error .envrc file not found";

        assert_eq!(TerminalText::new(raw).plain(), raw);
    }
}

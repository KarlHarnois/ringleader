use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct TmuxSequence {
    commands: Vec<Vec<OsString>>,
}

impl TmuxSequence {
    const SEPARATOR: &'static str = ";";
    const FORMAT_MARKER: u8 = b'#';
    const STYLE_OPENER: u8 = b'[';

    pub fn new() -> Self {
        Self::default()
    }

    pub fn command<I, W>(mut self, words: I) -> Self
    where
        I: IntoIterator<Item = W>,
        W: AsRef<OsStr>,
    {
        let escaped = words
            .into_iter()
            .map(|word| Self::escape_trailing_separator(word.as_ref()))
            .collect();
        self.commands.push(escaped);
        self
    }

    pub fn first_subcommand(&self) -> String {
        self.commands
            .first()
            .and_then(|words| words.first())
            .map(|word| word.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn starts_server(&self) -> bool {
        self.commands
            .iter()
            .any(|words| words.first().is_some_and(|word| word == "new-session"))
    }

    pub fn argv(&self) -> Vec<OsString> {
        let mut argv = Vec::new();
        for (index, words) in self.commands.iter().enumerate() {
            if index > 0 {
                argv.push(OsString::from(Self::SEPARATOR));
            }
            argv.extend(words.iter().cloned());
        }
        argv
    }

    pub fn start_directory(path: &Path) -> OsString {
        let bytes = path.as_os_str().as_bytes();
        let mut escaped = Vec::with_capacity(bytes.len() * 2);
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] != Self::FORMAT_MARKER {
                escaped.push(bytes[index]);
                index += 1;
                continue;
            }
            let run_end = Self::run_end(bytes, index);
            let run = &bytes[index..run_end];
            escaped.extend_from_slice(run);
            if bytes.get(run_end) != Some(&Self::STYLE_OPENER) {
                escaped.extend_from_slice(run);
            }
            index = run_end;
        }
        OsString::from_vec(escaped)
    }

    fn run_end(bytes: &[u8], start: usize) -> usize {
        bytes[start..]
            .iter()
            .position(|&byte| byte != Self::FORMAT_MARKER)
            .map_or(bytes.len(), |offset| start + offset)
    }

    fn escape_trailing_separator(word: &OsStr) -> OsString {
        let bytes = word.as_bytes();
        match bytes.split_last() {
            Some((b';', head)) => {
                let mut escaped = head.to_vec();
                escaped.extend_from_slice(b"\\;");
                OsString::from_vec(escaped)
            }
            _ => word.to_os_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_joined_by_standalone_separators() {
        let sequence = TmuxSequence::new()
            .command(["kill-session", "-t", "=a:"])
            .command(["display-message", "-p", "done"]);

        assert_eq!(
            sequence.argv(),
            [
                "kill-session",
                "-t",
                "=a:",
                ";",
                "display-message",
                "-p",
                "done"
            ]
        );
    }

    #[test]
    fn a_trailing_semicolon_is_escaped() {
        let sequence = TmuxSequence::new().command(["set-option", "@x", "fix bug;"]);

        assert_eq!(sequence.argv()[2], r"fix bug\;");
    }

    #[test]
    fn an_escaped_trailing_semicolon_gains_another_backslash() {
        let sequence = TmuxSequence::new().command(["set-option", "@x", r"a\;"]);

        assert_eq!(sequence.argv()[2], r"a\\;");
    }

    #[test]
    fn an_inner_semicolon_stays() {
        let sequence = TmuxSequence::new().command(["set-option", "@x", "x ; y"]);

        assert_eq!(sequence.argv()[2], "x ; y");
    }

    #[test]
    fn a_format_opener_in_a_start_directory_is_doubled() {
        let escaped = TmuxSequence::start_directory(Path::new("/tmp/a#{b}"));

        assert_eq!(escaped, "/tmp/a##{b}");
    }

    #[test]
    fn a_run_followed_by_a_style_opener_stays_as_written() {
        let escaped = TmuxSequence::start_directory(Path::new("/tmp/##[x]/#[#{y}"));

        assert_eq!(escaped, "/tmp/##[x]/#[##{y}");
    }

    #[test]
    fn a_trailing_marker_is_doubled() {
        let escaped = TmuxSequence::start_directory(Path::new("/tmp/x#"));

        assert_eq!(escaped, "/tmp/x##");
    }
}

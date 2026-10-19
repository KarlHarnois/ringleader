pub struct TmuxCommandLine<'a> {
    words: &'a [String],
}

impl<'a> TmuxCommandLine<'a> {
    const VALUE_FLAGS: [char; 5] = ['S', 'f', 'L', 'c', 'T'];
    const SOCKET_FLAG: char = 'S';

    pub fn new(words: &'a [String]) -> Self {
        Self { words }
    }

    pub fn socket(&self) -> Option<&'a str> {
        self.global_values()
            .into_iter()
            .rev()
            .find_map(|(flag, value)| (flag == Self::SOCKET_FLAG).then_some(value))
    }

    fn global_values(&self) -> Vec<(char, &'a str)> {
        let mut values = Vec::new();
        let mut words = self.words.iter().skip(1).map(String::as_str);
        while let Some(flags) = words.next().and_then(Self::flag_cluster) {
            if let Some((flag, attached)) = Self::value_flag(flags) {
                let value = match attached {
                    "" => words.next().unwrap_or_default(),
                    _ => attached,
                };
                values.push((flag, value));
            }
        }
        values
    }

    fn flag_cluster(word: &str) -> Option<&str> {
        word.strip_prefix('-')
            .filter(|flags| !flags.is_empty() && !flags.starts_with('-'))
    }

    fn value_flag(flags: &str) -> Option<(char, &str)> {
        let position = flags.find(Self::VALUE_FLAGS)?;
        let flag = flags[position..].chars().next()?;
        Some((flag, &flags[position + flag.len_utf8()..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CommandLineFixture;

    impl CommandLineFixture {
        fn words(words: &[&str]) -> Vec<String> {
            words.iter().map(|word| word.to_string()).collect()
        }
    }

    #[test]
    fn the_socket_is_read_from_flag_clusters_and_attached_values() {
        let words = CommandLineFixture::words(&[
            "tmux",
            "-uNS",
            "/run/example/tmux.sock",
            "-f/dev/null",
            "new-session",
        ]);

        assert_eq!(
            TmuxCommandLine::new(&words).socket(),
            Some("/run/example/tmux.sock")
        );
    }

    #[test]
    fn a_socket_flag_after_the_command_word_belongs_to_the_command() {
        let words = CommandLineFixture::words(&[
            "tmux",
            "-L",
            "example",
            "new-session",
            "sh",
            "-c",
            "sleep 30",
            "-S",
            "/run/example/tmux.sock",
        ]);

        assert_eq!(TmuxCommandLine::new(&words).socket(), None);
    }
}

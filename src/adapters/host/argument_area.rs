pub struct ArgumentArea {
    bytes: Vec<u8>,
}

impl ArgumentArea {
    const COUNT_SIZE: usize = size_of::<libc::c_int>();

    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    pub fn arguments(&self) -> Vec<String> {
        self.strings_after_executable().take(self.count()).collect()
    }

    #[cfg(test)]
    pub fn environment(&self) -> Vec<String> {
        self.strings_after_executable()
            .skip(self.count())
            .take_while(|variable| !variable.is_empty())
            .collect()
    }

    fn count(&self) -> usize {
        self.bytes
            .first_chunk::<{ Self::COUNT_SIZE }>()
            .map(|count| libc::c_int::from_ne_bytes(*count))
            .and_then(|count| usize::try_from(count).ok())
            .unwrap_or_default()
    }

    fn strings_after_executable(&self) -> impl Iterator<Item = String> + '_ {
        let after_count = self.bytes.get(Self::COUNT_SIZE..).unwrap_or_default();
        let executable = after_count.iter().take_while(|byte| **byte != 0).count();
        let padding = after_count[executable..]
            .iter()
            .take_while(|byte| **byte == 0)
            .count();
        after_count[executable + padding..]
            .split(|byte| *byte == 0)
            .map(|string| String::from_utf8_lossy(string).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AreaFixture;

    impl AreaFixture {
        fn area(count: libc::c_int, strings: &str) -> ArgumentArea {
            let mut bytes = count.to_ne_bytes().to_vec();
            bytes.extend_from_slice(strings.as_bytes());
            ArgumentArea::new(bytes)
        }

        fn server() -> ArgumentArea {
            Self::area(
                3,
                "/usr/bin/tmux\0\0\0\0tmux\0-S\0/tmp/s\0PATH=/bin\0HOME=/h\0\0ptr_munge=\0",
            )
        }
    }

    #[test]
    fn the_arguments_follow_the_executable_and_its_padding() {
        assert_eq!(AreaFixture::server().arguments(), ["tmux", "-S", "/tmp/s"]);
    }

    #[test]
    fn the_environment_follows_the_arguments_up_to_an_empty_string() {
        assert_eq!(
            AreaFixture::server().environment(),
            ["PATH=/bin", "HOME=/h"]
        );
    }

    #[test]
    fn an_empty_area_holds_no_arguments() {
        assert!(ArgumentArea::new(Vec::new()).arguments().is_empty());
    }

    #[test]
    fn a_zeroed_area_holds_no_arguments() {
        assert!(ArgumentArea::new(vec![0; 64]).arguments().is_empty());
    }

    #[test]
    fn an_area_cut_short_keeps_the_arguments_it_holds() {
        let area = AreaFixture::area(3, "/usr/bin/tmux\0tmux\0-S");

        assert_eq!(area.arguments(), ["tmux", "-S"]);
    }

    #[test]
    fn a_negative_count_holds_no_arguments() {
        let area = AreaFixture::area(-1, "/usr/bin/tmux\0tmux\0");

        assert!(area.arguments().is_empty());
    }
}

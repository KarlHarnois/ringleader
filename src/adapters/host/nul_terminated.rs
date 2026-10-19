use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

pub struct NulTerminated;

impl NulTerminated {
    pub fn text(characters: &[libc::c_char]) -> String {
        String::from_utf8_lossy(&Self::bytes(characters)).into_owned()
    }

    pub fn path(characters: &[libc::c_char]) -> Option<PathBuf> {
        let bytes = Self::bytes(characters);
        (!bytes.is_empty()).then(|| PathBuf::from(OsString::from_vec(bytes)))
    }

    fn bytes(characters: &[libc::c_char]) -> Vec<u8> {
        characters
            .iter()
            .map(|character| u8::from_ne_bytes(character.to_ne_bytes()))
            .take_while(|byte| *byte != 0)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    struct CharactersFixture;

    impl CharactersFixture {
        fn of(bytes: &[u8]) -> Vec<libc::c_char> {
            bytes
                .iter()
                .map(|byte| libc::c_char::from_ne_bytes(byte.to_ne_bytes()))
                .collect()
        }
    }

    #[test]
    fn the_text_ends_at_the_first_nul() {
        let characters = CharactersFixture::of(b"/usr/bin/tmux\0leftover\0");

        assert_eq!(NulTerminated::text(&characters), "/usr/bin/tmux");
    }

    #[test]
    fn text_without_a_nul_runs_to_the_end() {
        let characters = CharactersFixture::of(b"/usr/bin/tmux");

        assert_eq!(NulTerminated::text(&characters), "/usr/bin/tmux");
    }

    #[test]
    fn a_path_keeps_bytes_that_are_not_utf8() {
        let characters = CharactersFixture::of(b"/home/user/donn\xe9es\0");

        let path = NulTerminated::path(&characters).unwrap();

        assert_eq!(path.into_os_string().into_vec(), b"/home/user/donn\xe9es");
    }

    #[test]
    fn a_zeroed_field_is_no_path() {
        assert_eq!(NulTerminated::path(&[0; 16]), None);
    }

    #[test]
    fn a_set_field_is_its_path() {
        let characters = CharactersFixture::of(b"/Users/example\0");

        assert_eq!(
            NulTerminated::path(&characters).as_deref(),
            Some(Path::new("/Users/example"))
        );
    }
}

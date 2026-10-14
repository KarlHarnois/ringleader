use std::path::{Path, PathBuf};

use thiserror::Error;

pub trait DirectoryEnvironment: Send + Sync {
    fn envrc(&self, directory: &Path) -> Option<Envrc>;
    fn allow(&self, directory: &Path) -> Result<(), DirectoryEnvironmentError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envrc {
    contents: Option<Vec<u8>>,
    allowed: bool,
    allow_command: String,
}

impl Envrc {
    pub fn new(contents: Option<Vec<u8>>, allow_command: &str) -> Self {
        Self {
            contents,
            allowed: false,
            allow_command: allow_command.to_string(),
        }
    }

    pub fn allowed(self) -> Self {
        Self {
            allowed: true,
            ..self
        }
    }

    pub fn is_allowed(&self) -> bool {
        self.allowed
    }

    pub fn has_contents_of(&self, other: &Envrc) -> bool {
        self.contents.is_some() && self.contents == other.contents
    }

    pub fn allow_command(&self) -> &str {
        &self.allow_command
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DirectoryEnvironmentError {
    #[error("cannot allow the .envrc of {directory}: {reason}")]
    AllowFailed { directory: PathBuf, reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvrcFixture;

    impl EnvrcFixture {
        const ALLOW: &'static str = "direnv allow /repos/example-project";

        fn reading(contents: &str) -> Envrc {
            Envrc::new(Some(contents.as_bytes().to_vec()), Self::ALLOW)
        }

        fn unreadable() -> Envrc {
            Envrc::new(None, Self::ALLOW)
        }
    }

    #[test]
    fn an_envrc_with_the_same_bytes_has_the_contents_of_the_other() {
        let envrc = EnvrcFixture::reading("use flake\n");

        assert!(envrc.has_contents_of(&EnvrcFixture::reading("use flake\n")));
    }

    #[test]
    fn one_differing_byte_makes_different_contents() {
        let envrc = EnvrcFixture::reading("use flake\n");

        assert!(!envrc.has_contents_of(&EnvrcFixture::reading("use flake \n")));
    }

    #[test]
    fn two_unreadable_envrcs_never_have_the_same_contents() {
        let envrc = EnvrcFixture::unreadable();

        assert!(!envrc.has_contents_of(&EnvrcFixture::unreadable()));
    }

    #[test]
    fn an_envrc_is_not_allowed_until_marked() {
        let envrc = EnvrcFixture::reading("use flake\n");

        assert!(!envrc.is_allowed());
        assert!(envrc.allowed().is_allowed());
    }
}

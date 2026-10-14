use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::application::ports::AgentLaunch;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffContents {
    argv: Vec<Vec<u8>>,
    inherited: Vec<(Vec<u8>, Vec<u8>)>,
    explicit: Vec<(Vec<u8>, Vec<u8>)>,
    worktree: Vec<u8>,
    envrc: EnvrcStep,
}

impl HandoffContents {
    pub fn for_launch(
        launch: &AgentLaunch,
        inherited: &[(OsString, OsString)],
        explicit: &[(OsString, OsString)],
    ) -> Self {
        Self {
            argv: launch
                .argv()
                .iter()
                .map(|word| word.as_bytes().to_vec())
                .collect(),
            inherited: Self::stored(inherited),
            explicit: Self::stored(explicit),
            worktree: launch.worktree().as_os_str().as_bytes().to_vec(),
            envrc: EnvrcStep::for_launch(launch),
        }
    }

    pub fn inside_envrc(&self) -> Self {
        Self {
            inherited: Vec::new(),
            envrc: EnvrcStep::Loaded,
            ..self.clone()
        }
    }

    pub fn argv(&self) -> Vec<OsString> {
        self.argv
            .iter()
            .map(|word| OsString::from_vec(word.clone()))
            .collect()
    }

    pub fn inherited(&self) -> Vec<(OsString, OsString)> {
        Self::restored(&self.inherited)
    }

    pub fn explicit(&self) -> Vec<(OsString, OsString)> {
        Self::restored(&self.explicit)
    }

    pub fn worktree(&self) -> PathBuf {
        PathBuf::from(OsStr::from_bytes(&self.worktree))
    }

    pub fn envrc(&self) -> EnvrcStep {
        self.envrc
    }

    fn stored(variables: &[(OsString, OsString)]) -> Vec<(Vec<u8>, Vec<u8>)> {
        variables
            .iter()
            .map(|(name, value)| (name.as_bytes().to_vec(), value.as_bytes().to_vec()))
            .collect()
    }

    fn restored(variables: &[(Vec<u8>, Vec<u8>)]) -> Vec<(OsString, OsString)> {
        variables
            .iter()
            .map(|(name, value)| {
                (
                    OsString::from_vec(name.clone()),
                    OsString::from_vec(value.clone()),
                )
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnvrcStep {
    Skip,
    Load,
    Loaded,
}

impl EnvrcStep {
    fn for_launch(launch: &AgentLaunch) -> Self {
        match launch.loads_envrc() {
            true => Self::Load,
            false => Self::Skip,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    struct ContentsFixture;

    impl ContentsFixture {
        fn launch() -> AgentLaunch {
            AgentLaunch::new(
                vec!["claude".to_string()],
                &[],
                Path::new("/worktrees/fix-auth"),
            )
        }

        fn pair(name: &str, value: &str) -> (OsString, OsString) {
            (OsString::from(name), OsString::from(value))
        }

        fn contents(launch: &AgentLaunch) -> HandoffContents {
            HandoffContents::for_launch(
                launch,
                &[Self::pair("PATH", "/bin")],
                &[Self::pair("TMPDIR", "/scratch/fix-auth")],
            )
        }
    }

    #[test]
    fn a_plain_launch_skips_the_envrc() {
        let contents = ContentsFixture::contents(&ContentsFixture::launch());

        assert_eq!(contents.envrc(), EnvrcStep::Skip);
    }

    #[test]
    fn a_launch_through_the_envrc_hands_the_load_step() {
        let launch = ContentsFixture::launch().loading_envrc();

        assert_eq!(ContentsFixture::contents(&launch).envrc(), EnvrcStep::Load);
    }

    #[test]
    fn the_contents_inside_the_envrc_keep_only_the_explicit_values() {
        let contents = ContentsFixture::contents(&ContentsFixture::launch().loading_envrc());

        let inside = contents.inside_envrc();

        assert_eq!(inside.envrc(), EnvrcStep::Loaded);
        assert!(inside.inherited().is_empty());
        assert_eq!(inside.explicit(), contents.explicit());
        assert_eq!(
            (inside.argv(), inside.worktree()),
            (contents.argv(), contents.worktree())
        );
    }
}

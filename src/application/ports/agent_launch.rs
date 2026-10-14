use std::path::{Path, PathBuf};

use crate::domain::Agent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLaunch {
    argv: Vec<String>,
    env_names: Vec<String>,
    env_assignments: Vec<(String, String)>,
    worktree: PathBuf,
}

impl AgentLaunch {
    const TEMP_DIRECTORY_VARIABLE: &'static str = "TMPDIR";

    pub fn new(argv: Vec<String>, env_names: &[String], worktree: &Path) -> Self {
        Self {
            argv,
            env_names: env_names.to_vec(),
            env_assignments: Vec::new(),
            worktree: worktree.to_path_buf(),
        }
    }

    pub fn for_agent(agent: &Agent, argv: Vec<String>, worktree: &Path) -> Self {
        Self::new(argv, agent.env_names(), worktree).with_env_assignments(agent.env_assignments())
    }

    pub fn with_env_assignments(self, assignments: &[(String, String)]) -> Self {
        Self {
            env_assignments: assignments.to_vec(),
            ..self
        }
    }

    pub fn with_temp_directory(mut self, directory: &Path) -> Self {
        if !self.assigns(Self::TEMP_DIRECTORY_VARIABLE) {
            let value = directory.to_string_lossy().into_owned();
            self.env_assignments
                .push((Self::TEMP_DIRECTORY_VARIABLE.to_string(), value));
        }
        self
    }

    fn assigns(&self, variable: &str) -> bool {
        self.env_assignments
            .iter()
            .any(|(name, _)| name == variable)
    }

    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    pub fn env_names(&self) -> &[String] {
        &self.env_names
    }

    pub fn env_assignments(&self) -> &[(String, String)] {
        &self.env_assignments
    }

    pub fn worktree(&self) -> &Path {
        &self.worktree
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LaunchFixture;

    impl LaunchFixture {
        const WORKTREE: &'static str = "/data/ringleader/worktrees/example-project/fix-auth";
        const SCRATCH: &'static str = "/tmp/ringleader-1000/779a3f80f082ab8d";

        fn assigning(assignments: &[(&str, &str)]) -> AgentLaunch {
            let assignments: Vec<(String, String)> = assignments
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect();
            AgentLaunch::new(Vec::new(), &[], Path::new(Self::WORKTREE))
                .with_env_assignments(&assignments)
        }

        fn pair(name: &str, value: &str) -> (String, String) {
            (name.to_string(), value.to_string())
        }
    }

    #[test]
    fn a_temp_directory_is_assigned_to_tmpdir() {
        let launch = LaunchFixture::assigning(&[("EXAMPLE_MODE", "fast")])
            .with_temp_directory(Path::new(LaunchFixture::SCRATCH));

        assert_eq!(
            launch.env_assignments(),
            [
                LaunchFixture::pair("EXAMPLE_MODE", "fast"),
                LaunchFixture::pair("TMPDIR", LaunchFixture::SCRATCH),
            ]
        );
    }

    #[test]
    fn a_tmpdir_the_agent_sets_wins_over_the_temp_directory() {
        let launch = LaunchFixture::assigning(&[("TMPDIR", "/scratch/agent")])
            .with_temp_directory(Path::new(LaunchFixture::SCRATCH));

        assert_eq!(
            launch.env_assignments(),
            [LaunchFixture::pair("TMPDIR", "/scratch/agent")]
        );
    }
}

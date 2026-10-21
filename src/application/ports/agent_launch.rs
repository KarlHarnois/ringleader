use std::path::{Path, PathBuf};

use crate::domain::Agent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLaunch {
    argv: Vec<String>,
    env_names: Vec<String>,
    env_assignments: Vec<(String, String)>,
    worktree: PathBuf,
    loads_envrc: bool,
    offers_built_in_skills: bool,
}

impl AgentLaunch {
    const TEMP_DIRECTORY_VARIABLE: &'static str = "TMPDIR";

    pub fn new(argv: Vec<String>, env_names: &[String], worktree: &Path) -> Self {
        Self {
            argv,
            env_names: env_names.to_vec(),
            env_assignments: Vec::new(),
            worktree: worktree.to_path_buf(),
            loads_envrc: false,
            offers_built_in_skills: false,
        }
    }

    pub fn for_agent(agent: &Agent, argv: Vec<String>, worktree: &Path) -> Self {
        Self {
            offers_built_in_skills: agent.gets_built_in_skills(),
            ..Self::new(argv, agent.env_names(), worktree)
                .with_env_assignments(agent.env_assignments())
        }
    }

    #[cfg(test)]
    pub fn offering_built_in_skills(self) -> Self {
        Self {
            offers_built_in_skills: true,
            ..self
        }
    }

    pub fn with_env_assignments(self, assignments: &[(String, String)]) -> Self {
        Self {
            env_assignments: assignments.to_vec(),
            ..self
        }
    }

    pub fn with_temp_directory(mut self, directory: &Path) -> Self {
        if !self.sets_temp_directory() {
            let value = directory.to_string_lossy().into_owned();
            self.env_assignments
                .push((Self::TEMP_DIRECTORY_VARIABLE.to_string(), value));
        }
        self
    }

    pub fn loading_envrc(self) -> Self {
        Self {
            loads_envrc: true,
            ..self
        }
    }

    pub fn sets_temp_directory(&self) -> bool {
        self.env_assignments
            .iter()
            .any(|(name, _)| name == Self::TEMP_DIRECTORY_VARIABLE)
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

    pub fn loads_envrc(&self) -> bool {
        self.loads_envrc
    }

    pub fn offers_built_in_skills(&self) -> bool {
        self.offers_built_in_skills
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LaunchFixture;

    impl LaunchFixture {
        const WORKTREE: &'static str = "/data/ringleader/worktrees/example-project/fix-auth";
        const SCRATCH: &'static str = "/home/example/.cache/ringleader/tmp/8718940d";

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

    #[test]
    fn a_launch_for_an_agent_offers_the_built_in_skills_the_agent_gets() {
        let agent = Agent::new("claude", "claude");

        let launch = AgentLaunch::for_agent(&agent, Vec::new(), Path::new(LaunchFixture::WORKTREE));

        assert!(launch.offers_built_in_skills());
    }

    #[test]
    fn a_launch_for_an_agent_that_opts_out_offers_no_built_in_skills() {
        let agent = Agent::new("claude", "claude").without_built_in_skills();

        let launch = AgentLaunch::for_agent(&agent, Vec::new(), Path::new(LaunchFixture::WORKTREE));

        assert!(!launch.offers_built_in_skills());
    }
}

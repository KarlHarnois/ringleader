use std::path::{Path, PathBuf};

use crate::domain::RepoName;
use crate::domain::SessionName;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NamingScheme {
    worktree_root: PathBuf,
    tmux_prefix: String,
}

impl NamingScheme {
    const TMUX_PREFIX: &'static str = "ringleader-";

    pub fn for_new_sessions(worktree_root: &Path) -> Self {
        Self::new(worktree_root, Self::TMUX_PREFIX)
    }

    pub fn new(worktree_root: &Path, tmux_prefix: &str) -> Self {
        Self {
            worktree_root: worktree_root.to_path_buf(),
            tmux_prefix: tmux_prefix.to_string(),
        }
    }

    pub fn worktree_root(&self) -> &Path {
        &self.worktree_root
    }

    pub fn tmux_prefix(&self) -> &str {
        &self.tmux_prefix
    }

    pub fn tmux_session_name(&self, name: &SessionName) -> String {
        format!("{}{}", self.tmux_prefix, name)
    }

    pub fn branch_name(&self, name: &SessionName) -> String {
        name.to_string()
    }

    pub fn worktree_path(&self, repo: &RepoName, name: &SessionName) -> PathBuf {
        self.worktree_root.join(repo.as_str()).join(name.as_str())
    }

    pub fn strip_tmux_prefix<'a>(&self, tmux_name: &'a str) -> Option<&'a str> {
        tmux_name.strip_prefix(self.tmux_prefix.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NamingFixture;

    impl NamingFixture {
        fn default_scheme() -> NamingScheme {
            NamingScheme::new(Path::new("/worktrees"), "ringleader-")
        }
    }

    #[test]
    fn the_branch_is_the_session_name() {
        let branch =
            NamingFixture::default_scheme().branch_name(&SessionName::new("fix-auth").unwrap());

        assert_eq!(branch, "fix-auth");
    }

    #[test]
    fn the_worktree_path_nests_the_session_under_its_repo() {
        let path = NamingFixture::default_scheme().worktree_path(
            &RepoName::parse("example-project").unwrap(),
            &SessionName::new("fix-auth").unwrap(),
        );

        assert_eq!(path, Path::new("/worktrees/example-project/fix-auth"));
    }

    #[test]
    fn strips_the_tmux_prefix() {
        let scheme = NamingFixture::default_scheme();

        assert_eq!(
            scheme.strip_tmux_prefix("ringleader-fix-auth"),
            Some("fix-auth")
        );
    }

    #[test]
    fn a_name_without_the_tmux_prefix_strips_to_nothing() {
        let scheme = NamingFixture::default_scheme();

        assert_eq!(scheme.strip_tmux_prefix("work"), None);
    }
}

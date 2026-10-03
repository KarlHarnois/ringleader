use std::path::{Path, PathBuf};

use super::config::NamingScheme;
use super::repo_name::RepoName;
use super::session_metadata::SessionMetadata;
use super::session_name::SessionName;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    name: SessionName,
    metadata: SessionMetadata,
    registration: Registration,
}

impl Session {
    pub fn new(name: SessionName, metadata: SessionMetadata, registration: Registration) -> Self {
        Self {
            name,
            metadata,
            registration,
        }
    }

    pub fn name(&self) -> &SessionName {
        &self.name
    }

    pub fn metadata(&self) -> &SessionMetadata {
        &self.metadata
    }

    pub fn registration(&self) -> &Registration {
        &self.registration
    }

    pub fn repo_name(&self) -> &RepoName {
        self.metadata.repo_name()
    }

    pub fn repo_path(&self) -> &Path {
        self.metadata.repo_path()
    }

    pub fn worktree_path(&self) -> &Path {
        self.metadata.worktree_path()
    }

    pub fn branch(&self) -> &str {
        self.metadata.branch()
    }

    pub fn base_commit(&self) -> &str {
        self.metadata.base_commit()
    }

    pub fn agent(&self) -> &str {
        self.metadata.agent()
    }

    pub fn tmux_name(&self) -> &str {
        self.registration.tmux.session_name()
    }

    pub fn socket_path(&self) -> &Path {
        self.registration.tmux.socket_path()
    }

    pub fn created_at(&self) -> u64 {
        self.registration.created_at
    }

    pub fn naming(&self) -> &NamingScheme {
        &self.registration.naming
    }

    pub fn renamed(&self, name: SessionName, branch: &str) -> Self {
        let tmux_name = self.naming().tmux_session_name(&name);
        Self {
            metadata: self.metadata.with_branch(branch),
            registration: self
                .registration
                .on_tmux(TmuxAddress::new(&tmux_name, self.socket_path())),
            name,
        }
    }

    pub fn rekeyed(&self, name: SessionName, metadata: SessionMetadata, tmux: TmuxAddress) -> Self {
        Self {
            name,
            metadata,
            registration: self.registration.on_tmux(tmux),
        }
    }

    pub fn on_socket(&self, socket_path: &Path) -> Self {
        let tmux = TmuxAddress::new(self.tmux_name(), socket_path);
        Self {
            registration: self.registration.on_tmux(tmux),
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    tmux: TmuxAddress,
    naming: NamingScheme,
    created_at: u64,
}

impl Registration {
    pub fn new(tmux: TmuxAddress, naming: NamingScheme, created_at: u64) -> Self {
        Self {
            tmux,
            naming,
            created_at,
        }
    }

    fn on_tmux(&self, tmux: TmuxAddress) -> Self {
        Self {
            tmux,
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TmuxAddress {
    session_name: String,
    socket_path: PathBuf,
}

impl TmuxAddress {
    pub fn new(session_name: &str, socket_path: &Path) -> Self {
        Self {
            session_name: session_name.to_string(),
            socket_path: socket_path.to_path_buf(),
        }
    }

    pub fn session_name(&self) -> &str {
        &self.session_name
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::session_metadata::{SessionRepo, SessionWorktree};

    struct SessionFixture;

    impl SessionFixture {
        fn fix_auth() -> Session {
            let naming = NamingScheme::new(Path::new("/worktrees"), "ringleader/", "ringleader-");
            let metadata = SessionMetadata::new(
                SessionRepo::new(
                    RepoName::parse("example-project").unwrap(),
                    Path::new("/repos/example-project"),
                ),
                SessionWorktree::new(
                    Path::new("/worktrees/example-project/fix-auth"),
                    "ringleader/fix-auth",
                    "abc123",
                ),
                "claude",
            );
            let tmux = TmuxAddress::new("ringleader-fix-auth", Path::new("/tmp/tmux-1000/default"));
            Session::new(
                SessionName::new("fix-auth").unwrap(),
                metadata,
                Registration::new(tmux, naming, 1_700_000_000),
            )
        }
    }

    #[test]
    fn renaming_derives_the_tmux_name_from_the_recorded_prefix() {
        let session = SessionFixture::fix_auth();

        let renamed = session.renamed(
            SessionName::new("fix-login").unwrap(),
            "ringleader/fix-login",
        );

        assert_eq!(renamed.tmux_name(), "ringleader-fix-login");
        assert_eq!(renamed.branch(), "ringleader/fix-login");
    }

    #[test]
    fn renaming_keeps_the_worktree_path() {
        let session = SessionFixture::fix_auth();

        let renamed = session.renamed(
            SessionName::new("fix-login").unwrap(),
            "ringleader/fix-login",
        );

        assert_eq!(renamed.worktree_path(), session.worktree_path());
    }

    #[test]
    fn moving_to_another_socket_keeps_the_created_timestamp() {
        let session = SessionFixture::fix_auth();

        let moved = session.on_socket(Path::new("/tmp/other/default"));

        assert_eq!(moved.socket_path(), Path::new("/tmp/other/default"));
        assert_eq!(moved.created_at(), session.created_at());
    }
}

use std::path::{Path, PathBuf};

use super::session_metadata::SessionMetadata;
use super::session_name::SessionName;
use crate::domain::BaseBranch;
use crate::domain::NamingScheme;
use crate::domain::RepoName;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    name: SessionName,
    metadata: SessionMetadata,
    registration: Registration,
    favorite: bool,
}

impl Session {
    pub fn new(name: SessionName, metadata: SessionMetadata, registration: Registration) -> Self {
        Self {
            name,
            metadata,
            registration,
            favorite: false,
        }
    }

    pub fn name(&self) -> &SessionName {
        &self.name
    }

    pub fn metadata(&self) -> &SessionMetadata {
        &self.metadata
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

    pub fn requested_base_branch(&self) -> Option<&BaseBranch> {
        self.metadata.requested_base_branch()
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

    pub fn is_favorite(&self) -> bool {
        self.favorite
    }

    pub fn favorited(mut self) -> Self {
        self.favorite = true;
        self
    }

    pub fn unfavorited(mut self) -> Self {
        self.favorite = false;
        self
    }

    pub fn toggled_favorite(&self) -> Self {
        match self.favorite {
            true => self.clone().unfavorited(),
            false => self.clone().favorited(),
        }
    }

    pub fn has_branch_named_after_it(&self) -> bool {
        self.branch() == self.naming().branch_name(&self.name)
    }

    pub fn runs_in_main_checkout(&self) -> bool {
        self.metadata.runs_in_main_checkout()
    }

    pub fn allows_pull(&self) -> bool {
        !self.runs_in_main_checkout()
    }

    pub fn renamed(&self, name: SessionName) -> Self {
        let tmux_name = self.naming().tmux_session_name(&name);
        Self {
            metadata: self.renamed_metadata(&name),
            registration: self
                .registration
                .on_tmux(TmuxAddress::new(&tmux_name, self.socket_path())),
            name,
            favorite: self.favorite,
        }
    }

    fn renamed_metadata(&self, name: &SessionName) -> SessionMetadata {
        if self.runs_in_main_checkout() {
            return self.metadata.clone();
        }
        self.metadata.with_branch(&self.naming().branch_name(name))
    }

    pub fn rekeyed(&self, name: SessionName, metadata: SessionMetadata, tmux: TmuxAddress) -> Self {
        Self {
            name,
            metadata,
            registration: self.registration.on_tmux(tmux),
            favorite: self.favorite,
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
    use crate::domain::{SessionRepo, SessionWorktree};

    struct SessionFixture;

    impl SessionFixture {
        fn fix_auth() -> Session {
            let naming = NamingScheme::new(Path::new("/worktrees"), "ringleader-");
            let metadata = SessionMetadata::new(
                SessionRepo::new(
                    RepoName::parse("example-project").unwrap(),
                    Path::new("/repos/example-project"),
                ),
                SessionWorktree::new(
                    Path::new("/worktrees/example-project/fix-auth"),
                    "fix-auth",
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

        fn fix_login() -> SessionName {
            SessionName::new("fix-login").unwrap()
        }

        fn in_main_checkout() -> Session {
            let session = Self::fix_auth();
            let metadata = SessionMetadata::new(
                SessionRepo::new(session.repo_name().clone(), session.repo_path()),
                SessionWorktree::new(session.repo_path(), "main", "abc123"),
                "claude",
            );
            Session::new(session.name().clone(), metadata, session.registration)
        }
    }

    #[test]
    fn renaming_derives_the_tmux_name_from_the_recorded_prefix() {
        let session = SessionFixture::fix_auth();

        let renamed = session.renamed(SessionFixture::fix_login());

        assert_eq!(renamed.tmux_name(), "ringleader-fix-login");
    }

    #[test]
    fn renaming_names_the_branch_after_the_new_name() {
        let session = SessionFixture::fix_auth();

        let renamed = session.renamed(SessionFixture::fix_login());

        assert_eq!(renamed.branch(), "fix-login");
        assert!(renamed.has_branch_named_after_it());
    }

    #[test]
    fn a_branch_from_before_the_prefix_was_dropped_is_not_named_after_the_session() {
        let session = SessionFixture::fix_auth();
        let legacy = Session::new(
            session.name().clone(),
            session.metadata().with_branch("ringleader/fix-auth"),
            session.registration.clone(),
        );

        assert!(!legacy.has_branch_named_after_it());
    }

    #[test]
    fn renaming_keeps_the_worktree_path() {
        let session = SessionFixture::fix_auth();

        let renamed = session.renamed(SessionFixture::fix_login());

        assert_eq!(renamed.worktree_path(), session.worktree_path());
    }

    #[test]
    fn renaming_a_session_in_the_main_checkout_keeps_the_checked_out_branch() {
        let session = SessionFixture::in_main_checkout();

        let renamed = session.renamed(SessionFixture::fix_login());

        assert_eq!(renamed.branch(), "main");
    }

    #[test]
    fn a_session_in_the_main_checkout_allows_no_pull() {
        assert!(!SessionFixture::in_main_checkout().allows_pull());
    }

    #[test]
    fn a_session_in_its_own_worktree_allows_a_pull() {
        assert!(SessionFixture::fix_auth().allows_pull());
    }

    #[test]
    fn a_new_session_is_not_a_favorite() {
        assert!(!SessionFixture::fix_auth().is_favorite());
    }

    #[test]
    fn toggling_a_favorite_changes_it_each_time() {
        let session = SessionFixture::fix_auth();

        let favorited = session.toggled_favorite();
        let restored = favorited.toggled_favorite();

        assert!(favorited.is_favorite());
        assert!(!restored.is_favorite());
    }

    #[test]
    fn renaming_keeps_the_favorite() {
        let session = SessionFixture::fix_auth().favorited();

        let renamed = session.renamed(SessionFixture::fix_login());

        assert!(renamed.is_favorite());
    }

    #[test]
    fn rekeying_keeps_the_favorite() {
        let session = SessionFixture::fix_auth().favorited();
        let tmux = TmuxAddress::new(session.tmux_name(), session.socket_path());

        let rekeyed = session.rekeyed(session.name().clone(), session.metadata().clone(), tmux);

        assert!(rekeyed.is_favorite());
    }

    #[test]
    fn moving_to_another_socket_keeps_the_favorite() {
        let session = SessionFixture::fix_auth().favorited();

        let moved = session.on_socket(Path::new("/tmp/other/default"));

        assert!(moved.is_favorite());
    }

    #[test]
    fn moving_to_another_socket_keeps_the_created_timestamp() {
        let session = SessionFixture::fix_auth();

        let moved = session.on_socket(Path::new("/tmp/other/default"));

        assert_eq!(moved.socket_path(), Path::new("/tmp/other/default"));
        assert_eq!(moved.created_at(), session.created_at());
    }
}

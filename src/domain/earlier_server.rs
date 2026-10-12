use std::path::{Path, PathBuf};

use super::session::Session;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EarlierServer {
    socket: PathBuf,
    tmux_name: String,
}

impl EarlierServer {
    pub fn of(session: &Session, private_socket: &Path) -> Option<Self> {
        (session.socket_path() != private_socket).then(|| Self {
            socket: session.socket_path().to_path_buf(),
            tmux_name: session.tmux_name().to_string(),
        })
    }

    pub fn refusal(&self, name: &str) -> String {
        format!(
            "{name} runs on the tmux server at {}, which Ringleader no longer uses. Attach there with {}, or end it with {} and restart it here.",
            self.socket.display(),
            self.attach_command(),
            self.kill_command()
        )
    }

    pub fn cleanup_note(&self) -> String {
        format!(
            "The earlier tmux session may still sit on {} as an exited pane, which Ringleader cannot see. End it with {}.",
            self.socket.display(),
            self.kill_command()
        )
    }

    pub fn audit(&self, name: &str, worktree_processes: &str) -> String {
        format!(
            "session {name} was created on the tmux server at {}, which Ringleader no longer uses, and {worktree_processes}. An exited session kept as a pane leaves no process, so check with tmux -S {} ls. Attach there with {}, or end it with {} and run ringleader restart {name}.",
            self.socket.display(),
            self.socket.display(),
            self.attach_command(),
            self.kill_command()
        )
    }

    fn attach_command(&self) -> String {
        format!(
            "tmux -S {} attach -t '={}:'",
            self.socket.display(),
            self.tmux_name
        )
    }

    fn kill_command(&self) -> String {
        format!(
            "tmux -S {} kill-session -t '={}:'",
            self.socket.display(),
            self.tmux_name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        NamingScheme, Registration, RepoName, SessionMetadata, SessionName, SessionRepo,
        SessionWorktree, TmuxAddress,
    };

    struct ServerFixture;

    impl ServerFixture {
        const PRIVATE: &'static str = "/run/user/1000/ringleader/tmux/tmux.sock";
        const EARLIER: &'static str = "/tmp/tmux-1000/default";

        fn session_on(socket: &str) -> Session {
            let metadata = SessionMetadata::new(
                SessionRepo::new(
                    RepoName::parse("example-project").unwrap(),
                    Path::new("/repos/example-project"),
                ),
                SessionWorktree::new(
                    Path::new("/worktrees/example-project/fix-auth"),
                    "ringleader/fix-auth",
                    "",
                ),
                "claude",
            );
            let naming = NamingScheme::new(Path::new("/worktrees"), "ringleader/", "ringleader-");
            let tmux = TmuxAddress::new("ringleader-fix-auth", Path::new(socket));
            Session::new(
                SessionName::new("fix-auth").unwrap(),
                metadata,
                Registration::new(tmux, naming, 0),
            )
        }
    }

    #[test]
    fn a_session_on_the_private_socket_has_no_earlier_server() {
        let session = ServerFixture::session_on(ServerFixture::PRIVATE);

        assert_eq!(
            EarlierServer::of(&session, Path::new(ServerFixture::PRIVATE)),
            None
        );
    }

    #[test]
    fn the_refusal_gives_the_attach_and_kill_commands_for_the_earlier_server() {
        let session = ServerFixture::session_on(ServerFixture::EARLIER);

        let refusal = EarlierServer::of(&session, Path::new(ServerFixture::PRIVATE))
            .unwrap()
            .refusal("fix-auth");

        assert!(
            refusal.contains("tmux -S /tmp/tmux-1000/default attach -t '=ringleader-fix-auth:'")
        );
        assert!(
            refusal
                .contains("tmux -S /tmp/tmux-1000/default kill-session -t '=ringleader-fix-auth:'")
        );
    }
}

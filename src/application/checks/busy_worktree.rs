use std::path::Path;

use crate::application::ports::{HostProbe, ProcessInfo};
use crate::domain::Session;

pub struct BusyWorktree<'a> {
    host: &'a dyn HostProbe,
    private_socket: &'a Path,
}

impl<'a> BusyWorktree<'a> {
    pub fn new(host: &'a dyn HostProbe, private_socket: &'a Path) -> Self {
        Self {
            host,
            private_socket,
        }
    }

    pub fn finding(&self, session: &Session) -> Option<String> {
        self.on_recorded_server(session)
            .or_else(|| self.on_private_server(session))
    }

    fn on_recorded_server(&self, session: &Session) -> Option<String> {
        let recorded = session.socket_path();
        if recorded == self.private_socket {
            return None;
        }
        let processes = self.processes_under(session, recorded)?;
        let server = format!("its tmux server at {}", recorded.display());
        Some(format!(
            "{} {} {}",
            Self::still_running(session, &server),
            Self::recorded_server_advice(session),
            Self::listed(&processes)
        ))
    }

    pub fn recorded_server_advice(session: &Session) -> String {
        format!(
            "{}, or run Ringleader again in the environment that started that server, which had another XDG_RUNTIME_DIR.",
            Self::kill_advice(session)
        )
    }

    pub fn kill_advice(session: &Session) -> String {
        format!(
            "End the session with tmux -S {} kill-session -t '={}:'",
            session.socket_path().display(),
            session.tmux_name()
        )
    }

    pub fn lost_socket_advice(socket: &Path) -> String {
        let directory = socket.parent().unwrap_or(socket);
        format!(
            "the socket {} was removed under the running server, and its sessions come back with it. Run ringleader again, or press r in a running TUI, to recreate it. By hand, recreate {} with mode 0700 and send SIGUSR1 to the tmux server whose command line names that socket.",
            socket.display(),
            directory.display()
        )
    }

    fn on_private_server(&self, session: &Session) -> Option<String> {
        let processes = self.processes_under(session, self.private_socket)?;
        let advice = match self.host.path_exists(self.private_socket) {
            true => "End them before a restart or rename.".to_string(),
            false => format!(
                "Do not end them: {}",
                Self::lost_socket_advice(self.private_socket)
            ),
        };
        Some(format!(
            "{} {advice} {}",
            Self::still_running(session, "Ringleader's tmux server"),
            Self::listed(&processes)
        ))
    }

    fn processes_under(&self, session: &Session, socket: &Path) -> Option<Vec<ProcessInfo>> {
        let processes = self
            .host
            .processes_under_server(session.worktree_path(), socket);
        (!processes.is_empty()).then_some(processes)
    }

    fn still_running(session: &Session, server: &str) -> String {
        format!(
            "session {} is stopped, but {server} still runs processes in its worktree.",
            session.name()
        )
    }

    fn listed(processes: &[ProcessInfo]) -> String {
        format!("Processes: {}.", ProcessInfo::listed(processes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FakeHostProbe, FakeMultiplexer, ServiceFixture};

    struct BusyFixture;

    impl BusyFixture {
        const FALLBACK_SOCKET: &'static str =
            "/home/example/.local/state/ringleader/tmux/tmux.sock";

        fn session() -> Session {
            ServiceFixture::session("fix-auth")
        }

        fn finding(host: &FakeHostProbe, session: &Session) -> Option<String> {
            BusyWorktree::new(host, Path::new(FakeMultiplexer::SOCKET)).finding(session)
        }
    }

    #[test]
    fn an_agent_on_the_server_of_another_runtime_directory_gets_its_kill_command() {
        let session = BusyFixture::session().on_socket(Path::new(BusyFixture::FALLBACK_SOCKET));
        let host = ServiceFixture::with_worktree_process(&session).serving(
            ServiceFixture::WORKTREE_PROCESS,
            Path::new(BusyFixture::FALLBACK_SOCKET),
        );

        let finding = BusyFixture::finding(&host, &session).unwrap();

        assert!(finding.contains(&format!(
            "tmux -S {} kill-session -t '=ringleader-fix-auth:'",
            BusyFixture::FALLBACK_SOCKET
        )));
    }

    #[test]
    fn an_agent_of_a_server_whose_socket_is_gone_gets_the_socket_fix_and_is_kept() {
        let session = BusyFixture::session();
        let host = ServiceFixture::with_worktree_agent(&session);

        let finding = BusyFixture::finding(&host, &session).unwrap();

        assert!(finding.contains("send SIGUSR1"));
        assert!(!finding.contains("End them"));
    }

    #[test]
    fn the_advice_comes_before_the_process_list() {
        let session = BusyFixture::session();
        let host = ServiceFixture::with_worktree_agent(&session);

        let finding = BusyFixture::finding(&host, &session).unwrap();

        let advice = finding.find("Do not end them").unwrap();
        let processes = finding
            .find(&ServiceFixture::WORKTREE_PROCESS.to_string())
            .unwrap();
        assert!(advice < processes);
    }

    #[test]
    fn a_process_of_a_server_still_on_its_socket_must_end_first() {
        let session = BusyFixture::session();
        let host = ServiceFixture::with_worktree_agent(&session)
            .with_file(Path::new(FakeMultiplexer::SOCKET));

        let finding = BusyFixture::finding(&host, &session).unwrap();

        assert!(finding.contains("End them before a restart or rename"));
        assert!(!finding.contains("send SIGUSR1"));
    }

    #[test]
    fn an_editor_outside_any_tmux_server_leaves_the_worktree_free() {
        let session = BusyFixture::session();
        let host = ServiceFixture::with_worktree_process(&session);

        assert_eq!(BusyFixture::finding(&host, &session), None);
    }
}

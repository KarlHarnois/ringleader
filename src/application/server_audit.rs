use std::path::Path;
use std::sync::Arc;

use super::diagnostic_paths::DiagnosticPaths;
use super::doctor_report::DoctorReport;
use super::ports::{HostProbe, LeftoverBinding, LeftoverBindings, Multiplexer, ServerState};
use super::service_ports::ServicePorts;
use super::tool_version::ToolVersion;
use crate::domain::ReturnKey;

pub struct ServerAudit {
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
    leftover_bindings: Arc<dyn LeftoverBindings>,
    paths: DiagnosticPaths,
}

impl ServerAudit {
    pub const MINIMUM_TMUX: (u32, u32) = (3, 2);
    const PRIVATE_MODE: u32 = 0o700;

    pub fn new(ports: &ServicePorts, paths: &DiagnosticPaths) -> Self {
        Self {
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
            leftover_bindings: ports.tmux().leftover_bindings(),
            paths: paths.clone(),
        }
    }

    pub fn audit(
        &self,
        return_key: Option<&ReturnKey>,
        client_version: Option<&str>,
        report: &mut DoctorReport,
    ) {
        self.check_location(report);
        self.check_directory(report);
        self.check_state(client_version, report);
        if let Some(return_key) = return_key {
            self.check_return_key(return_key, report);
            self.check_drift(return_key, report);
        }
        self.check_leftover_bindings(report);
    }

    fn check_location(&self, report: &mut DoctorReport) {
        let socket = self.paths.tmux_socket().display();
        report.info(format!(
            "Ringleader's tmux server: socket {socket}, config {}; inspect it with tmux -S {socket} ls",
            self.paths.tmux_config().display()
        ));
        if self.paths.socket_in_state_directory() {
            report.info("XDG_RUNTIME_DIR is unset or relative, so the tmux socket lives under the state directory");
        }
        if let Some(problem) = self.paths.socket_problem() {
            report.problem(problem);
        }
        if let Some(problem) = self.paths.user_tmux_config_problem() {
            report.warning(problem);
        }
    }

    fn check_directory(&self, report: &mut DoctorReport) {
        let socket = self.paths.tmux_socket();
        let directory = socket.parent().unwrap_or(socket);
        let Some(access) = self.host.directory_access(directory) else {
            return;
        };
        if access.mode() != Self::PRIVATE_MODE {
            report.problem(format!(
                "{} has mode {:o}, but Ringleader's tmux directory must have mode 700; run chmod 700 {}",
                directory.display(),
                access.mode(),
                directory.display()
            ));
        }
        if !access.owned_by_user() {
            report.problem(format!(
                "{} belongs to another user, so Ringleader's tmux socket there is not private; remove it",
                directory.display()
            ));
        }
    }

    fn check_state(&self, client_version: Option<&str>, report: &mut DoctorReport) {
        let socket = self.paths.tmux_socket().display();
        match self.multiplexer.server_state() {
            Ok(Some(state)) => Self::report_running(&state, client_version, report),
            Ok(None) => {
                report.info("Ringleader's tmux server is not running, the next session starts it")
            }
            Err(error) => report.problem(format!(
                "cannot reach Ringleader's tmux server at {socket}: {error}"
            )),
        }
    }

    fn report_running(state: &ServerState, client: Option<&str>, report: &mut DoctorReport) {
        let version = state.version();
        ToolVersion::new(version).audit("tmux server", Self::MINIMUM_TMUX, report);
        let sessions = match state.sessions() {
            1 => "1 session".to_string(),
            count => format!("{count} sessions"),
        };
        report.info(format!("Ringleader's tmux server holds {sessions}"));
        if let Some(client) = client.filter(|client| *client != version) {
            report.warning(format!(
                "Ringleader's tmux server runs tmux {version} but the client is tmux {client}, so the server predates a tmux upgrade; once its sessions are idle, end it with kill-server so the next session starts a new one"
            ));
        }
    }

    fn check_return_key(&self, return_key: &ReturnKey, report: &mut DoctorReport) {
        let key = return_key.display();
        if return_key.shadows_agent_input() {
            report.warning(format!(
                "return_key {return_key} is a key agents need, and in a session it detaches instead of reaching the agent; pick a key such as C-q"
            ));
        }
        if self.multiplexer.caller_in_other_tmux() {
            report.info(format!(
                "this runs inside your own tmux, so sessions open nested; your tmux must not use {key} as its prefix or bind it in its root table, which doctor cannot check without reading your tmux server"
            ));
        }
    }

    fn check_drift(&self, return_key: &ReturnKey, report: &mut DoctorReport) {
        let Ok(Some(running)) = self.multiplexer.server_config(return_key) else {
            return;
        };
        if !running.is_current() {
            report.warning("Ringleader's tmux server runs an older generated config; the TUI at start and after r, and ringleader new and restart, apply the current one");
        }
    }

    fn check_leftover_bindings(&self, report: &mut DoctorReport) {
        let path = self.leftover_bindings.path();
        match self.leftover_bindings.read() {
            Ok(bindings) => {
                for binding in bindings {
                    report.warning(Self::leftover_advice(&path, &binding));
                }
            }
            Err(error) => report.warning(format!("leftover Ctrl-Q bindings: {error}")),
        }
    }

    fn leftover_advice(path: &Path, binding: &LeftoverBinding) -> String {
        let file = path.display();
        let socket = binding.socket().display();
        let fix = match binding.saved_line() {
            Some(line) => format!(
                "it saved your binding {line} from the tmux server at {socket}. If C-q there still runs Ringleader's binding, put that line in a file and run tmux -S {socket} source-file <file>"
            ),
            None => format!(
                "the tmux server at {socket} had no C-q binding before. If C-q there still runs Ringleader's binding, run tmux -S {socket} unbind-key -T root C-q"
            ),
        };
        format!(
            "{file} is left from an earlier Ringleader that bound C-q on your tmux server: {fix}. Ringleader no longer changes that server, so delete {file} once it is done"
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::application::ports::{DirectoryAccess, LeftoverBindingsError, MultiplexerError};
    use crate::test_support::{
        FakeHostProbe, FakeLeftoverBindings, FakeMultiplexer, ServiceFixture,
    };

    struct AuditFixture;

    impl AuditFixture {
        const SOCKET_DIRECTORY: &'static str = "/run/user/1000/ringleader/tmux";

        fn lines(fixture: &ServiceFixture) -> Vec<String> {
            fixture.unloaded_service().doctor().lines()
        }

        fn line_with(fixture: &ServiceFixture, prefix: &str, fragment: &str) -> bool {
            Self::lines(fixture)
                .iter()
                .any(|line| line.starts_with(prefix) && line.contains(fragment))
        }

        fn with_directory(access: DirectoryAccess) -> ServiceFixture {
            ServiceFixture::new().with_host(
                FakeHostProbe::default()
                    .with_directory_access(Path::new(Self::SOCKET_DIRECTORY), access),
            )
        }

        fn with_return_key(key: &str) -> ServiceFixture {
            let draft = crate::domain::ConfigDraft::new(Path::new(ServiceFixture::WORKTREE_ROOT))
                .return_key(key)
                .agent(ServiceFixture::agent());
            ServiceFixture::new().with_config(crate::domain::Config::new(&draft).unwrap())
        }

        fn with_socket(socket: &str) -> ServiceFixture {
            ServiceFixture::new().with_diagnostics(
                ServiceFixture::diagnostics()
                    .with_tmux_server(Path::new(socket), Path::new(ServiceFixture::TMUX_CONFIG)),
            )
        }
    }

    #[test]
    fn the_location_names_the_socket_the_config_and_the_hand_command() {
        let fixture = ServiceFixture::new();

        assert!(AuditFixture::line_with(
            &fixture,
            "info:",
            &format!(
                "socket {}, config {}; inspect it with tmux -S {} ls",
                FakeMultiplexer::SOCKET,
                ServiceFixture::TMUX_CONFIG,
                FakeMultiplexer::SOCKET
            )
        ));
    }

    #[test]
    fn the_state_directory_fallback_is_an_info_line() {
        let fixture = ServiceFixture::new()
            .with_diagnostics(ServiceFixture::diagnostics().with_socket_in_state_directory());

        assert!(AuditFixture::line_with(
            &fixture,
            "info:",
            "XDG_RUNTIME_DIR is unset"
        ));
    }

    #[test]
    fn a_socket_path_with_a_comma_is_a_problem() {
        let fixture = AuditFixture::with_socket("/run/user/1000,x/ringleader/tmux/tmux.sock");

        assert!(AuditFixture::line_with(
            &fixture,
            "problem:",
            "contains a comma"
        ));
    }

    #[test]
    fn a_socket_path_of_a_hundred_bytes_is_a_problem() {
        let socket = format!("/{}/tmux.sock", "x".repeat(89));
        let fixture = AuditFixture::with_socket(&socket);

        assert!(AuditFixture::line_with(&fixture, "problem:", "100 bytes"));
    }

    #[test]
    fn a_config_home_with_a_bracket_draws_the_user_file_warning() {
        let fixture = ServiceFixture::new().with_diagnostics(
            ServiceFixture::diagnostics()
                .with_user_tmux_config(Path::new("/home/example/[1]/ringleader/tmux.conf")),
        );

        assert!(AuditFixture::line_with(
            &fixture,
            "warning:",
            "cannot load /home/example/[1]"
        ));
    }

    #[test]
    fn a_socket_directory_open_to_others_is_a_problem() {
        let fixture = AuditFixture::with_directory(DirectoryAccess::new(0o755, true));

        assert!(AuditFixture::line_with(
            &fixture,
            "problem:",
            "has mode 755"
        ));
    }

    #[test]
    fn a_socket_directory_of_another_user_is_a_problem() {
        let fixture = AuditFixture::with_directory(DirectoryAccess::new(0o700, false));

        assert!(AuditFixture::line_with(
            &fixture,
            "problem:",
            "belongs to another user"
        ));
    }

    #[test]
    fn a_private_socket_directory_draws_no_problem() {
        let fixture = AuditFixture::with_directory(DirectoryAccess::new(0o700, true));

        assert!(!AuditFixture::line_with(
            &fixture,
            "problem:",
            AuditFixture::SOCKET_DIRECTORY
        ));
    }

    #[test]
    fn a_stopped_server_is_an_info_line() {
        let fixture = ServiceFixture::new()
            .with_multiplexer(FakeMultiplexer::new().with_versions("3.6a", None));

        assert!(AuditFixture::line_with(&fixture, "info:", "is not running"));
    }

    #[test]
    fn a_running_server_reports_its_version_and_sessions() {
        let lines = AuditFixture::lines(&ServiceFixture::new());

        assert!(lines.contains(&"info: tmux server 3.6a".to_string()));
        assert!(lines.contains(&"info: Ringleader's tmux server holds 1 session".to_string()));
    }

    #[test]
    fn a_server_older_than_its_client_is_a_warning() {
        let fixture = ServiceFixture::new()
            .with_multiplexer(FakeMultiplexer::new().with_versions("3.6a", Some("3.5a")));

        assert!(AuditFixture::line_with(
            &fixture,
            "warning:",
            "predates a tmux upgrade"
        ));
    }

    #[test]
    fn a_server_that_cannot_be_reached_is_a_problem() {
        let fixture = ServiceFixture::new().with_multiplexer(FakeMultiplexer::new().failing(
            "server_state",
            MultiplexerError::Failed {
                subcommand: "display-message".to_string(),
                detail: "error connecting to the socket (Permission denied)".to_string(),
            },
        ));

        assert!(AuditFixture::line_with(
            &fixture,
            "problem:",
            "Permission denied"
        ));
    }

    #[test]
    fn a_return_key_agents_need_is_a_warning() {
        let fixture = AuditFixture::with_return_key("C-c");

        assert!(AuditFixture::line_with(
            &fixture,
            "warning:",
            "return_key C-c is a key agents need"
        ));
    }

    #[test]
    fn inside_another_tmux_the_return_key_needs_an_unbound_outer_key() {
        let fixture =
            ServiceFixture::new().with_multiplexer(FakeMultiplexer::new().called_from_other_tmux());

        assert!(AuditFixture::line_with(
            &fixture,
            "info:",
            "your tmux must not use Ctrl-Q as its prefix or bind it in its root table"
        ));
    }

    #[test]
    fn a_server_running_an_older_render_is_a_warning() {
        let fixture = ServiceFixture::new()
            .with_multiplexer(FakeMultiplexer::new().with_drifted_config("M-x"));

        assert!(AuditFixture::line_with(
            &fixture,
            "warning:",
            "older generated config"
        ));
    }

    #[test]
    fn a_server_running_the_current_render_draws_no_drift_warning() {
        assert!(!AuditFixture::line_with(
            &ServiceFixture::new(),
            "warning:",
            "older generated config"
        ));
    }

    #[test]
    fn each_leftover_binding_is_a_warning_with_its_socket_and_hand_command() {
        let fixture =
            ServiceFixture::new().with_leftover_bindings(FakeLeftoverBindings::with(vec![
                LeftoverBinding::new(
                    Path::new("/tmp/tmux-1000/default"),
                    Some("bind-key -T root C-q send-keys example"),
                ),
                LeftoverBinding::new(Path::new("/tmp/tmux-1000/work"), None),
            ]));

        assert!(AuditFixture::line_with(
            &fixture,
            "warning:",
            "your binding bind-key -T root C-q send-keys example from the tmux server at /tmp/tmux-1000/default"
        ));
        assert!(AuditFixture::line_with(
            &fixture,
            "warning:",
            "tmux -S /tmp/tmux-1000/work unbind-key -T root C-q"
        ));
        assert!(
            fixture
                .multiplexer()
                .calls()
                .iter()
                .all(|call| !call.contains("/tmp/tmux-1000/work"))
        );
    }

    #[test]
    fn an_unreadable_bindings_file_is_a_warning() {
        let fixture = ServiceFixture::new().with_leftover_bindings(FakeLeftoverBindings::failing(
            LeftoverBindingsError::Corrupt {
                path: PathBuf::from(FakeLeftoverBindings::PATH),
                detail: "expected value".to_string(),
            },
        ));

        assert!(AuditFixture::line_with(
            &fixture,
            "warning:",
            "is not valid JSON"
        ));
    }
}

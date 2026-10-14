use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use super::host_path::HostPath;
use super::live_process::LiveProcess;
use super::process_environment::ProcessEnvironment;
use crate::application::ports::{DirectoryAccess, HostProbe, HostProbeError, ProcessInfo};

pub struct SystemHostProbe {
    search_path: OsString,
    own_lineage: Vec<u32>,
}

impl SystemHostProbe {
    const TERMINATE_LIMIT: Duration = Duration::from_secs(5);
    const TERMINATE_POLL: Duration = Duration::from_millis(50);
    const EXECUTABLE_BITS: u32 = 0o111;
    const ACCESS_BITS: u32 = 0o777;

    pub fn new(environment: &ProcessEnvironment) -> Self {
        Self {
            search_path: environment
                .get("PATH")
                .map(OsString::from)
                .unwrap_or_default(),
            own_lineage: LiveProcess::lineage_of(std::process::id()),
        }
    }

    fn is_executable(path: &Path) -> bool {
        fs::metadata(path).is_ok_and(|metadata| {
            metadata.is_file() && metadata.permissions().mode() & Self::EXECUTABLE_BITS != 0
        })
    }

    pub fn user_id() -> u32 {
        // SAFETY: geteuid takes no arguments, reads no memory of this process, and cannot fail.
        unsafe { libc::geteuid() }
    }

    fn is_own(&self, process: &LiveProcess) -> bool {
        self.own_lineage.contains(&process.pid()) || process.parent() == std::process::id()
    }

    fn others_in<'p>(
        &'p self,
        processes: &'p [LiveProcess],
        path: &Path,
    ) -> impl Iterator<Item = &'p LiveProcess> {
        let canonical = HostPath::new(path).canonical_lenient();
        processes
            .iter()
            .filter(move |process| process.cwd().is_some_and(|cwd| cwd.starts_with(&canonical)))
            .filter(|process| !self.is_own(process) && !process.is_tmux())
    }

    fn probe_directory_write(directory: &Path) -> bool {
        let probe = directory.join(format!(".ringleader-write-probe-{}", std::process::id()));
        let created = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
            .is_ok();
        if created {
            let _ = fs::remove_file(&probe);
        }
        created
    }
}

impl HostProbe for SystemHostProbe {
    fn resolve_command(&self, command: &str) -> Option<PathBuf> {
        if command.contains('/') {
            let path = PathBuf::from(command);
            return (path.is_absolute() && Self::is_executable(&path)).then_some(path);
        }
        std::env::split_paths(&self.search_path)
            .map(|directory| directory.join(command))
            .find(|candidate| Self::is_executable(candidate))
    }

    fn is_writable(&self, path: &Path) -> bool {
        match fs::metadata(path) {
            Ok(metadata) if metadata.is_dir() => Self::probe_directory_write(path),
            Ok(_) => OpenOptions::new().write(true).open(path).is_ok(),
            Err(_) => false,
        }
    }

    fn path_exists(&self, path: &Path) -> bool {
        fs::metadata(path).is_ok()
    }

    fn is_file(&self, path: &Path) -> bool {
        fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
    }

    fn directory_access(&self, path: &Path) -> Option<DirectoryAccess> {
        let metadata = fs::metadata(path)
            .ok()
            .filter(|metadata| metadata.is_dir())?;
        let mode = metadata.permissions().mode() & Self::ACCESS_BITS;
        Some(DirectoryAccess::new(
            mode,
            metadata.uid() == Self::user_id(),
        ))
    }

    fn canonicalize_lenient(&self, path: &Path) -> Result<PathBuf, HostProbeError> {
        let refused = !path.is_absolute()
            || path
                .components()
                .any(|component| component == Component::ParentDir);
        if refused {
            return Err(HostProbeError::RelativePath {
                path: path.to_path_buf(),
            });
        }
        Ok(HostPath::new(path).canonical_lenient())
    }

    fn processes_in(&self, path: &Path, excluded_sessions: &[u32]) -> Vec<ProcessInfo> {
        let processes = LiveProcess::all();
        self.others_in(&processes, path)
            .filter(|process| !excluded_sessions.contains(&process.session()))
            .map(LiveProcess::info)
            .collect()
    }

    fn processes_under_server(&self, path: &Path, socket: &Path) -> Vec<ProcessInfo> {
        let socket = socket.to_string_lossy();
        let processes = LiveProcess::all();
        let by_pid: HashMap<u32, &LiveProcess> = processes
            .iter()
            .map(|process| (process.pid(), process))
            .collect();
        self.others_in(&processes, path)
            .filter(|process| process.runs_under_server(&by_pid, &socket))
            .map(LiveProcess::info)
            .collect()
    }

    fn session_processes(&self, sessions: &[u32]) -> Vec<ProcessInfo> {
        LiveProcess::all()
            .into_iter()
            .filter(|process| sessions.contains(&process.session()))
            .map(|process| process.info())
            .collect()
    }

    fn terminate(&self, pids: &[u32]) -> Vec<ProcessInfo> {
        let targets: Vec<LiveProcess> = pids
            .iter()
            .filter_map(|pid| LiveProcess::read(*pid))
            .collect();
        for process in &targets {
            let _ = process.signal(libc::SIGTERM);
            let _ = process.signal(libc::SIGCONT);
        }
        let deadline = Instant::now() + Self::TERMINATE_LIMIT;
        loop {
            let remaining: Vec<&LiveProcess> = targets
                .iter()
                .filter(|process| process.is_alive())
                .collect();
            if remaining.is_empty() || Instant::now() >= deadline {
                return remaining.into_iter().map(LiveProcess::info).collect();
            }
            thread::sleep(Self::TERMINATE_POLL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TmuxTestServer;

    struct ProbeFixture;

    impl ProbeFixture {
        fn probe() -> SystemHostProbe {
            let path = std::env::var_os("PATH").unwrap_or_default();
            SystemHostProbe::new(&ProcessEnvironment::new(vec![("PATH".into(), path)], None))
        }

        fn pid_of(server: &TmuxTestServer, pattern: &str) -> Option<u32> {
            ProbeFixture::probe()
                .processes_in(server.directory(), &[])
                .iter()
                .find(|process| process.command() == pattern)
                .map(ProcessInfo::pid)
        }

        fn nohup_child(server: &TmuxTestServer) -> u32 {
            let pane_pid: u32 = server
                .text(&["display-message", "-p", "-t", "=probe:", "#{pane_pid}"])
                .parse()
                .unwrap();
            ProbeFixture::probe()
                .session_processes(&[pane_pid])
                .iter()
                .find(|process| process.command() == "sleep 31")
                .map(ProcessInfo::pid)
                .expect("the nohup child runs in the pane's session")
        }

        fn started_probe_session() -> TmuxTestServer {
            let server = TmuxTestServer::new();
            let directory = server.directory().to_string_lossy().into_owned();
            server.text(&[
                "new-session",
                "-d",
                "-s",
                "probe",
                "-c",
                &directory,
                "sh",
                "-c",
                "nohup sleep 31 >/dev/null 2>&1 & sleep 30",
            ]);
            assert!(server.wait_for(|| ProbeFixture::pid_of(&server, "sleep 31").is_some()));
            server
        }
    }

    #[test]
    fn a_bare_command_resolves_on_the_snapshot_path() {
        let resolved = ProbeFixture::probe().resolve_command("sh").unwrap();

        assert!(resolved.is_absolute());
    }

    #[test]
    fn an_unknown_command_does_not_resolve() {
        assert_eq!(
            ProbeFixture::probe().resolve_command("no-such-agent-command"),
            None
        );
    }

    #[test]
    fn a_relative_command_with_a_slash_does_not_resolve() {
        assert_eq!(ProbeFixture::probe().resolve_command("bin/sh"), None);
    }

    #[test]
    fn a_directory_of_this_user_reports_its_mode() {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o750)).unwrap();

        let access = ProbeFixture::probe()
            .directory_access(directory.path())
            .unwrap();

        assert_eq!(access, DirectoryAccess::new(0o750, true));
    }

    #[test]
    fn processes_under_a_server_are_found_by_its_socket_and_not_by_another() {
        let server = ProbeFixture::started_probe_session();
        let other_socket = server.directory().join("other.sock");

        let own = ProbeFixture::probe().processes_under_server(server.directory(), server.socket());
        let other = ProbeFixture::probe().processes_under_server(server.directory(), &other_socket);

        assert!(own.iter().any(|process| process.command() == "sleep 31"));
        assert!(other.is_empty());
    }

    #[test]
    fn a_process_outside_any_tmux_server_is_not_under_one() {
        let directory = tempfile::tempdir().unwrap();
        std::process::Command::new("sh")
            .args(["-c", "nohup sleep 33 >/dev/null 2>&1 &"])
            .current_dir(directory.path())
            .status()
            .unwrap();
        let probe = ProbeFixture::probe();
        let orphan = probe.processes_in(directory.path(), &[]);

        let under = probe.processes_under_server(directory.path(), Path::new("/run/example.sock"));

        probe.terminate(&orphan.iter().map(ProcessInfo::pid).collect::<Vec<_>>());
        assert_eq!(orphan.len(), 1);
        assert!(under.is_empty());
    }

    #[test]
    fn a_sticky_bit_is_left_out_of_the_reported_mode() {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o1700)).unwrap();

        let access = ProbeFixture::probe()
            .directory_access(directory.path())
            .unwrap();

        assert_eq!(access.mode(), 0o700);
    }

    #[test]
    fn a_missing_directory_reports_no_access() {
        let directory = tempfile::tempdir().unwrap();

        let access = ProbeFixture::probe().directory_access(&directory.path().join("missing"));

        assert_eq!(access, None);
    }

    #[test]
    fn a_writable_directory_reads_as_writable_and_keeps_no_probe() {
        let directory = tempfile::tempdir().unwrap();

        assert!(ProbeFixture::probe().is_writable(directory.path()));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_relative_path_is_refused_by_lenient_canonicalization() {
        let result = ProbeFixture::probe().canonicalize_lenient(Path::new("relative/path"));

        assert!(result.is_err());
    }

    #[test]
    fn processes_in_lists_the_nohup_child_and_skips_the_tmux_server() {
        let server = ProbeFixture::started_probe_session();

        let commands: Vec<String> = ProbeFixture::probe()
            .processes_in(server.directory(), &[])
            .iter()
            .map(|process| process.command().to_string())
            .collect();

        assert!(commands.contains(&"sleep 31".to_string()));
        assert!(commands.iter().all(|command| !command.starts_with("tmux")));
    }

    #[test]
    fn session_processes_lists_the_nohup_child_of_the_pane() {
        let server = ProbeFixture::started_probe_session();

        assert!(ProbeFixture::nohup_child(&server) > 0);
    }

    #[test]
    fn terminate_stops_a_child_that_survived_kill_session() {
        let server = ProbeFixture::started_probe_session();
        let child = ProbeFixture::nohup_child(&server);
        server.text(&["set-option", "-g", "exit-empty", "off"]);
        server.text(&["kill-session", "-t", "=probe:"]);
        std::thread::sleep(Duration::from_millis(200));

        let survived_kill = LiveProcess::read(child).is_some_and(|process| process.is_alive());
        let survivors = ProbeFixture::probe().terminate(&[child]);

        assert!(survived_kill);
        assert!(survivors.is_empty());
    }
}

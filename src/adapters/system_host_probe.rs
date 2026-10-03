use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use super::host_path::HostPath;
use super::process_environment::ProcessEnvironment;
use crate::application::ports::{HostProbe, HostProbeError, ProcessInfo};

pub struct SystemHostProbe {
    search_path: OsString,
    own_lineage: Vec<u32>,
}

impl SystemHostProbe {
    const TERMINATE_LIMIT: Duration = Duration::from_secs(5);
    const TERMINATE_POLL: Duration = Duration::from_millis(50);
    const EXECUTABLE_BITS: u32 = 0o111;
    const DELETED_SUFFIX: &'static str = " (deleted)";

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

    fn is_own(&self, process: &LiveProcess) -> bool {
        self.own_lineage.contains(&process.pid) || process.parent == std::process::id()
    }

    fn is_tmux(process: &LiveProcess) -> bool {
        let executable = process.executable.trim_end_matches(Self::DELETED_SUFFIX);
        Path::new(executable)
            .file_name()
            .is_some_and(|name| name == "tmux")
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
        let canonical = HostPath::new(path).canonical_lenient();
        LiveProcess::all()
            .into_iter()
            .filter(|process| {
                process
                    .cwd
                    .as_ref()
                    .is_some_and(|cwd| cwd.starts_with(&canonical))
            })
            .filter(|process| !excluded_sessions.contains(&process.session))
            .filter(|process| !self.is_own(process) && !Self::is_tmux(process))
            .map(|process| process.info())
            .collect()
    }

    fn session_processes(&self, sessions: &[u32]) -> Vec<ProcessInfo> {
        LiveProcess::all()
            .into_iter()
            .filter(|process| sessions.contains(&process.session))
            .map(|process| process.info())
            .collect()
    }

    fn terminate(&self, pids: &[u32]) -> Vec<ProcessInfo> {
        let targets: Vec<LiveProcess> = pids
            .iter()
            .filter_map(|pid| LiveProcess::read(*pid))
            .collect();
        for process in &targets {
            let Ok(pid) = libc::pid_t::try_from(process.pid) else {
                continue;
            };
            // SAFETY: kill only sends signals to a pid read from /proc a moment ago; it
            // touches no memory, and a recycled pid at worst receives SIGTERM and SIGCONT.
            unsafe {
                libc::kill(pid, libc::SIGTERM);
                libc::kill(pid, libc::SIGCONT);
            }
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

struct LiveProcess {
    pid: u32,
    parent: u32,
    session: u32,
    cwd: Option<PathBuf>,
    executable: String,
    command: String,
}

impl LiveProcess {
    const ZOMBIE: &'static str = "Z";

    fn all() -> Vec<Self> {
        let Ok(entries) = fs::read_dir("/proc") else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_string_lossy().parse().ok())
            .filter_map(Self::read)
            .collect()
    }

    fn read(pid: u32) -> Option<Self> {
        let directory = PathBuf::from(format!("/proc/{pid}"));
        let stat = fs::read_to_string(directory.join("stat")).ok()?;
        let fields: Vec<&str> = stat[stat.rfind(')')? + 2..].split(' ').collect();
        let command_line = fs::read(directory.join("cmdline")).unwrap_or_default();
        Some(Self {
            pid,
            parent: fields.get(1)?.parse().ok()?,
            session: fields.get(3)?.parse().ok()?,
            cwd: fs::read_link(directory.join("cwd")).ok(),
            executable: fs::read_link(directory.join("exe"))
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            command: String::from_utf8_lossy(&command_line)
                .split('\0')
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" "),
        })
    }

    fn lineage_of(pid: u32) -> Vec<u32> {
        let mut lineage = Vec::new();
        let mut current = Some(pid);
        while let Some(pid) = current.filter(|&pid| pid > 0) {
            lineage.push(pid);
            current = Self::read(pid).map(|process| process.parent);
        }
        lineage
    }

    fn is_alive(&self) -> bool {
        let Ok(stat) = fs::read_to_string(format!("/proc/{}/stat", self.pid)) else {
            return false;
        };
        let state = stat.rfind(')').and_then(|end| stat.get(end + 2..end + 3));
        state != Some(Self::ZOMBIE)
    }

    fn info(&self) -> ProcessInfo {
        ProcessInfo::new(self.pid, &self.command)
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

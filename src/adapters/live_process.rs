use std::collections::HashMap;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::application::ports::ProcessInfo;

pub struct LiveProcess {
    pid: u32,
    parent: u32,
    session: u32,
    owner: u32,
    cwd: Option<PathBuf>,
    executable: String,
    arguments: Vec<String>,
    name: String,
}

impl LiveProcess {
    const ZOMBIE: &'static str = "Z";
    const DELETED_SUFFIX: &'static str = " (deleted)";
    const TMUX: &'static str = "tmux";
    const TMUX_SERVER_NAME: &'static str = "tmux: server";

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn parent(&self) -> u32 {
        self.parent
    }

    pub fn session(&self) -> u32 {
        self.session
    }

    pub fn cwd(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    pub fn is_tmux(&self) -> bool {
        let executable = self.executable.trim_end_matches(Self::DELETED_SUFFIX);
        Path::new(executable)
            .file_name()
            .is_some_and(|name| name == Self::TMUX)
    }

    pub fn runs_under_server(&self, by_pid: &HashMap<u32, &LiveProcess>, socket: &str) -> bool {
        std::iter::successors(by_pid.get(&self.parent), |ancestor| {
            by_pid.get(&ancestor.parent)
        })
        .take(by_pid.len())
        .any(|ancestor| ancestor.serves(socket))
    }

    pub fn tmux_servers_on(socket: &Path) -> Vec<Self> {
        let socket = socket.to_string_lossy();
        Self::all()
            .into_iter()
            .filter(|process| process.serves(&socket))
            .collect()
    }

    pub fn current_user() -> u32 {
        // SAFETY: geteuid takes no arguments, reads no memory of this process, and cannot fail.
        unsafe { libc::geteuid() }
    }

    pub fn signal(&self, signal: libc::c_int) -> io::Result<()> {
        let pid = libc::pid_t::try_from(self.pid)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        // SAFETY: kill only sends a signal to a pid read from /proc a moment ago and
        // touches no memory; a recycled pid at worst receives that signal.
        let sent = unsafe { libc::kill(pid, signal) };
        match sent {
            0 => Ok(()),
            _ => Err(io::Error::last_os_error()),
        }
    }

    fn serves(&self, socket: &str) -> bool {
        self.owner == Self::current_user()
            && self.name == Self::TMUX_SERVER_NAME
            && TmuxCommandLine::new(&self.arguments).socket() == Some(socket)
    }

    pub fn all() -> Vec<Self> {
        let Ok(entries) = fs::read_dir("/proc") else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_string_lossy().parse().ok())
            .filter_map(Self::read)
            .collect()
    }

    pub fn read(pid: u32) -> Option<Self> {
        let directory = PathBuf::from(format!("/proc/{pid}"));
        let stat = fs::read_to_string(directory.join("stat")).ok()?;
        let fields: Vec<&str> = stat[stat.rfind(')')? + 2..].split(' ').collect();
        let command_line = fs::read(directory.join("cmdline")).unwrap_or_default();
        Some(Self {
            pid,
            parent: fields.get(1)?.parse().ok()?,
            session: fields.get(3)?.parse().ok()?,
            owner: fs::metadata(&directory).ok()?.uid(),
            cwd: fs::read_link(directory.join("cwd")).ok(),
            executable: fs::read_link(directory.join("exe"))
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            arguments: String::from_utf8_lossy(&command_line)
                .split('\0')
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect(),
            name: fs::read_to_string(directory.join("comm"))
                .map(|name| name.trim_end().to_string())
                .unwrap_or_default(),
        })
    }

    pub fn lineage_of(pid: u32) -> Vec<u32> {
        let mut lineage = Vec::new();
        let mut current = Some(pid);
        while let Some(pid) = current.filter(|&pid| pid > 0) {
            lineage.push(pid);
            current = Self::read(pid).map(|process| process.parent);
        }
        lineage
    }

    pub fn is_alive(&self) -> bool {
        let Ok(stat) = fs::read_to_string(format!("/proc/{}/stat", self.pid)) else {
            return false;
        };
        let state = stat.rfind(')').and_then(|end| stat.get(end + 2..end + 3));
        state != Some(Self::ZOMBIE)
    }

    pub fn info(&self) -> ProcessInfo {
        ProcessInfo::new(self.pid, &self.arguments.join(" "))
    }
}

struct TmuxCommandLine<'a> {
    words: &'a [String],
}

impl<'a> TmuxCommandLine<'a> {
    const VALUE_FLAGS: [char; 5] = ['S', 'f', 'L', 'c', 'T'];
    const SOCKET_FLAG: char = 'S';

    fn new(words: &'a [String]) -> Self {
        Self { words }
    }

    fn socket(&self) -> Option<&'a str> {
        self.global_values()
            .into_iter()
            .rev()
            .find_map(|(flag, value)| (flag == Self::SOCKET_FLAG).then_some(value))
    }

    fn global_values(&self) -> Vec<(char, &'a str)> {
        let mut values = Vec::new();
        let mut words = self.words.iter().skip(1).map(String::as_str);
        while let Some(flags) = words.next().and_then(Self::flag_cluster) {
            if let Some((flag, attached)) = Self::value_flag(flags) {
                let value = match attached {
                    "" => words.next().unwrap_or_default(),
                    _ => attached,
                };
                values.push((flag, value));
            }
        }
        values
    }

    fn flag_cluster(word: &str) -> Option<&str> {
        word.strip_prefix('-')
            .filter(|flags| !flags.is_empty() && !flags.starts_with('-'))
    }

    fn value_flag(flags: &str) -> Option<(char, &str)> {
        let position = flags.find(Self::VALUE_FLAGS)?;
        let flag = flags[position..].chars().next()?;
        Some((flag, &flags[position + flag.len_utf8()..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TmuxTestServer;

    struct ProcessFixture;

    impl ProcessFixture {
        const SOCKET: &'static str = "/run/user/1000/ringleader/tmux/tmux.sock";
        const SERVER_PID: u32 = 4100;
        const AGENT_PID: u32 = 4101;

        const STARTING_ARGUMENTS: [&'static str; 4] = ["tmux", "-S", Self::SOCKET, "new-session"];

        fn server_owned_by(owner: u32) -> LiveProcess {
            Self::server_started_with(owner, &Self::STARTING_ARGUMENTS)
        }

        fn server_started_with(owner: u32, arguments: &[&str]) -> LiveProcess {
            LiveProcess {
                pid: Self::SERVER_PID,
                parent: 1,
                session: Self::SERVER_PID,
                owner,
                cwd: None,
                executable: "/usr/bin/tmux".to_string(),
                arguments: arguments.iter().map(|word| word.to_string()).collect(),
                name: "tmux: server".to_string(),
            }
        }

        fn agent_under(server: &LiveProcess) -> LiveProcess {
            LiveProcess {
                pid: Self::AGENT_PID,
                parent: server.pid,
                session: Self::AGENT_PID,
                owner: server.owner,
                cwd: None,
                executable: "/usr/bin/sleep".to_string(),
                arguments: ["sleep", "30"].map(String::from).to_vec(),
                name: "sleep".to_string(),
            }
        }
    }

    #[test]
    fn a_server_whose_command_only_mentions_a_socket_does_not_serve_it() {
        let ours = TmuxTestServer::with_session("holder");
        let other = TmuxTestServer::new();
        let socket = ours.socket().to_string_lossy();
        other.text(&[
            "new-session",
            "-d",
            "-s",
            "watch",
            "sh",
            "-c",
            "sleep 30",
            "-S",
            &socket,
        ]);

        let servers: Vec<u32> = LiveProcess::tmux_servers_on(ours.socket())
            .iter()
            .map(LiveProcess::pid)
            .collect();

        assert_eq!(servers, [ours.server_pid().parse::<u32>().unwrap()]);
    }

    #[test]
    fn a_server_of_another_user_does_not_serve_the_socket() {
        let server = ProcessFixture::server_owned_by(LiveProcess::current_user() + 1);
        let agent = ProcessFixture::agent_under(&server);
        let by_pid = HashMap::from([(server.pid, &server), (agent.pid, &agent)]);

        assert!(!agent.runs_under_server(&by_pid, ProcessFixture::SOCKET));
    }

    #[test]
    fn a_server_started_with_clustered_flags_serves_its_socket() {
        let arguments = [
            "tmux",
            "-uNS",
            ProcessFixture::SOCKET,
            "-f/dev/null",
            "new-session",
            "-S",
            "/run/example.sock",
        ];
        let server = ProcessFixture::server_started_with(LiveProcess::current_user(), &arguments);
        let agent = ProcessFixture::agent_under(&server);
        let by_pid = HashMap::from([(server.pid, &server), (agent.pid, &agent)]);

        assert!(agent.runs_under_server(&by_pid, ProcessFixture::SOCKET));
    }
}

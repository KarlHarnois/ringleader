use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::application::ports::ProcessInfo;

pub struct LiveProcess {
    pid: u32,
    parent: u32,
    session: u32,
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
    const SOCKET_FLAG: &'static str = "-S";

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
        self.name == Self::TMUX_SERVER_NAME
            && self
                .arguments
                .windows(2)
                .any(|pair| pair[0] == Self::SOCKET_FLAG && pair[1] == socket)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TmuxTestServer;

    #[test]
    fn a_server_whose_command_only_mentions_a_socket_does_not_serve_it() {
        let ours = TmuxTestServer::with_session("holder");
        let other = TmuxTestServer::new();
        let watching = format!("echo {}; sleep 30", ours.socket().display());
        other.text(&["new-session", "-d", "-s", "watch", &watching]);

        let servers: Vec<u32> = LiveProcess::tmux_servers_on(ours.socket())
            .iter()
            .map(LiveProcess::pid)
            .collect();

        assert_eq!(servers, [ours.server_pid().parse::<u32>().unwrap()]);
    }
}

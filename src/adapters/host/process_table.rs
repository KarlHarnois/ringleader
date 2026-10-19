use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::live_process::LiveProcess;

pub trait ProcessScope: Send + Sync {
    fn lists(&self, pid: u32) -> bool;
}

struct EveryProcess;

impl ProcessScope for EveryProcess {
    fn lists(&self, _pid: u32) -> bool {
        true
    }
}

#[derive(Clone)]
pub struct ProcessTable {
    scope: Arc<dyn ProcessScope>,
}

impl ProcessTable {
    const ROOT: &'static str = "/proc";

    pub fn new() -> Self {
        Self::within(Arc::new(EveryProcess))
    }

    pub fn within(scope: Arc<dyn ProcessScope>) -> Self {
        Self { scope }
    }

    pub fn tmux_servers_on(&self, socket: &Path) -> Vec<LiveProcess> {
        LiveProcess::servers_among(self.all(), socket)
    }

    pub fn all(&self) -> Vec<LiveProcess> {
        let Ok(entries) = fs::read_dir(Self::ROOT) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_string_lossy().parse().ok())
            .filter(|pid| self.scope.lists(*pid))
            .filter_map(|pid| self.read(pid))
            .collect()
    }

    pub fn lineage_of(&self, pid: u32) -> Vec<u32> {
        let mut lineage = Vec::new();
        let mut current = Some(pid);
        while let Some(pid) = current.filter(|&pid| pid > 0) {
            lineage.push(pid);
            current = self.read(pid).map(|process| process.parent());
        }
        lineage
    }

    pub fn is_alive(&self, pid: u32) -> bool {
        ProcessStat::read(pid).is_some_and(|stat| !stat.is_zombie())
    }

    pub fn read(&self, pid: u32) -> Option<LiveProcess> {
        let directory = Self::directory_of(pid);
        let stat = ProcessStat::read(pid)?;
        let owner = fs::metadata(&directory).ok()?.uid();
        let command_line = fs::read(directory.join("cmdline")).unwrap_or_default();
        let arguments = String::from_utf8_lossy(&command_line)
            .split('\0')
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect();
        let executable = fs::read_link(directory.join("exe"))
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = fs::read_to_string(directory.join("comm"))
            .map(|name| name.trim_end().to_string())
            .unwrap_or_default();
        Some(
            LiveProcess::new(pid, stat.parent()?, stat.session()?, owner)
                .with_cwd(fs::read_link(directory.join("cwd")).ok())
                .with_executable(&executable)
                .with_arguments(arguments)
                .with_name(&name),
        )
    }

    fn directory_of(pid: u32) -> PathBuf {
        Path::new(Self::ROOT).join(pid.to_string())
    }
}

struct ProcessStat {
    fields_after_name: Vec<String>,
}

impl ProcessStat {
    const NAME_END: char = ')';
    const ZOMBIE: &'static str = "Z";
    const STATE: usize = 0;
    const PARENT: usize = 1;
    const SESSION: usize = 3;

    fn read(pid: u32) -> Option<Self> {
        let stat = fs::read_to_string(ProcessTable::directory_of(pid).join("stat")).ok()?;
        let after_name = stat.get(stat.rfind(Self::NAME_END)? + 2..)?;
        Some(Self {
            fields_after_name: after_name.split(' ').map(str::to_string).collect(),
        })
    }

    fn is_zombie(&self) -> bool {
        self.fields_after_name.get(Self::STATE).map(String::as_str) == Some(Self::ZOMBIE)
    }

    fn parent(&self) -> Option<u32> {
        self.number(Self::PARENT)
    }

    fn session(&self) -> Option<u32> {
        self.number(Self::SESSION)
    }

    fn number(&self, index: usize) -> Option<u32> {
        self.fields_after_name.get(index)?.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Deadline, ProcessMarker};

    struct TableFixture;

    impl TableFixture {
        fn end(mut child: std::process::Child) {
            child.kill().unwrap();
            child.wait().unwrap();
        }
    }

    #[test]
    fn a_marked_table_lists_the_marked_process_and_not_the_test_itself() {
        let marker = ProcessMarker::new();
        let child = marker.sleeping_child();
        let table = marker.table();

        let listed: Vec<u32> = table.all().iter().map(LiveProcess::pid).collect();

        TableFixture::end(child);
        assert!(!listed.contains(&std::process::id()));
        assert_eq!(listed.len(), 1);
    }

    #[test]
    fn a_marked_table_still_reads_an_unmarked_process_by_pid() {
        let table = ProcessMarker::new().table();

        let own = table.read(std::process::id()).unwrap();

        assert_eq!(own.pid(), std::process::id());
    }

    #[test]
    fn the_lineage_starts_at_the_pid_and_ends_at_pid_one_without_the_kernel_parent() {
        let table = ProcessMarker::new().table();

        let lineage = table.lineage_of(std::process::id());

        assert_eq!(lineage.first(), Some(&std::process::id()));
        assert_eq!(lineage.last(), Some(&1));
        assert!(!lineage.contains(&0));
    }

    #[test]
    fn the_production_scope_lists_every_pid() {
        assert!(EveryProcess.lists(u32::MAX));
    }

    #[test]
    fn a_pid_that_no_process_has_is_not_alive() {
        let table = ProcessMarker::new().table();

        assert!(!table.is_alive(u32::MAX));
    }

    #[test]
    fn a_running_process_is_alive() {
        let table = ProcessMarker::new().table();

        assert!(table.is_alive(std::process::id()));
    }

    #[test]
    fn a_process_that_exited_but_was_not_waited_for_is_a_zombie_and_not_alive() {
        let marker = ProcessMarker::new();
        let mut child = marker.sleeping_child();
        child.kill().unwrap();
        let table = marker.table();
        let zombie = Deadline::for_background_work().wait_until(|| !table.is_alive(child.id()));

        child.wait().unwrap();
        assert!(zombie);
    }
}

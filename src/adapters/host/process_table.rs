use std::path::Path;
use std::sync::Arc;

use super::live_process::LiveProcess;
use crate::adapters::HostProcessTable;

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
        HostProcessTable::pids()
            .into_iter()
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
        HostProcessTable::is_alive(pid)
    }

    pub fn read(&self, pid: u32) -> Option<LiveProcess> {
        HostProcessTable::read(pid)
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

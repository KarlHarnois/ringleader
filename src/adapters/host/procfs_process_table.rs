use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::live_process::LiveProcess;
use crate::domain::{MemorySize, ProcessFigures};

pub struct ProcfsProcessTable;

impl ProcfsProcessTable {
    const ROOT: &'static str = "/proc";
    const SERVER_TITLE: &'static str = ": server (";
    const PROCESS_NAME_LIMIT: usize = 15;

    pub fn pids() -> Vec<u32> {
        Self::listed_pids().unwrap_or_default()
    }

    pub fn listed_pids() -> io::Result<Vec<u32>> {
        let entries = fs::read_dir(Self::ROOT)?;
        Ok(entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_string_lossy().parse().ok())
            .collect())
    }

    pub fn figures(pid: u32) -> Option<ProcessFigures> {
        let stat = ProcessStat::read(pid)?;
        Some(
            ProcessFigures::new(pid, stat.parent()?, stat.name())
                .with_cpu_time(stat.cpu_time()?)
                .with_resident_memory(stat.resident_memory()?),
        )
    }

    pub fn is_alive(pid: u32) -> bool {
        ProcessStat::read(pid).is_some_and(|stat| !stat.is_zombie())
    }

    pub fn read(pid: u32) -> Option<LiveProcess> {
        let directory = Self::directory_of(pid);
        let stat = ProcessStat::read(pid)?;
        let owner = fs::metadata(&directory).ok()?.uid();
        let command_line = fs::read(directory.join("cmdline")).unwrap_or_default();
        let arguments = Self::nul_separated(&command_line);
        let executable = fs::read_link(directory.join("exe"))
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = fs::read_to_string(directory.join("comm"))
            .map(|name| name.trim_end().to_string())
            .unwrap_or_default();
        let server_shape = name == Self::server_name(&arguments);
        Some(
            LiveProcess::new(pid, stat.parent()?, stat.session()?, owner)
                .with_cwd(fs::read_link(directory.join("cwd")).ok())
                .with_executable(&executable)
                .with_arguments(arguments)
                .with_server_shape(server_shape),
        )
    }

    #[cfg(test)]
    pub fn environment(pid: u32) -> Vec<String> {
        let environ = fs::read(Self::directory_of(pid).join("environ")).unwrap_or_default();
        Self::nul_separated(&environ)
    }

    fn directory_of(pid: u32) -> PathBuf {
        Path::new(Self::ROOT).join(pid.to_string())
    }

    fn nul_separated(bytes: &[u8]) -> Vec<String> {
        String::from_utf8_lossy(bytes)
            .split('\0')
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect()
    }

    fn server_name(arguments: &[String]) -> String {
        let title = format!("{}{}", Self::program_name(arguments), Self::SERVER_TITLE);
        let kept = title.get(..Self::PROCESS_NAME_LIMIT).unwrap_or(&title);
        Self::without_cut_word(kept).to_string()
    }

    fn program_name(arguments: &[String]) -> String {
        arguments
            .first()
            .and_then(|program| Path::new(program).file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn without_cut_word(kept: &str) -> &str {
        kept.rsplit_once(' ')
            .map_or(kept, |(whole_words, _)| whole_words)
    }

    fn configured(name: libc::c_int) -> Option<u64> {
        // SAFETY: sysconf only reads a system configuration value and touches no memory.
        let value = unsafe { libc::sysconf(name) };
        u64::try_from(value).ok().filter(|value| *value > 0)
    }
}

struct ProcessStat {
    name: String,
    fields_after_name: Vec<String>,
}

impl ProcessStat {
    const NAME_START: char = '(';
    const NAME_END: char = ')';
    const ZOMBIE: &'static str = "Z";
    const STATE: usize = 0;
    const PARENT: usize = 1;
    const SESSION: usize = 3;
    const USER_TICKS: usize = 11;
    const SYSTEM_TICKS: usize = 12;
    const RESIDENT_PAGES: usize = 21;
    const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;

    fn read(pid: u32) -> Option<Self> {
        let stat = fs::read_to_string(ProcfsProcessTable::directory_of(pid).join("stat")).ok()?;
        let name_start = stat.find(Self::NAME_START)? + 1;
        let name_end = stat.rfind(Self::NAME_END)?;
        let after_name = stat.get(name_end + 2..)?;
        Some(Self {
            name: stat.get(name_start..name_end)?.to_string(),
            fields_after_name: after_name.split(' ').map(str::to_string).collect(),
        })
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn cpu_time(&self) -> Option<Duration> {
        let ticks = self.count(Self::USER_TICKS)? + self.count(Self::SYSTEM_TICKS)?;
        let ticks_per_second = ProcfsProcessTable::configured(libc::_SC_CLK_TCK)?;
        let nanoseconds = ticks.saturating_mul(Self::NANOSECONDS_PER_SECOND) / ticks_per_second;
        Some(Duration::from_nanos(nanoseconds))
    }

    fn resident_memory(&self) -> Option<MemorySize> {
        let pages = self.count(Self::RESIDENT_PAGES)?;
        let page_size = ProcfsProcessTable::configured(libc::_SC_PAGESIZE)?;
        Some(MemorySize::of_bytes(pages.saturating_mul(page_size)))
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

    fn count(&self, index: usize) -> Option<u64> {
        self.fields_after_name.get(index)?.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Deadline, ProcessMarker};

    struct TableFixture;

    impl TableFixture {
        fn words(words: &[&str]) -> Vec<String> {
            words.iter().map(|word| word.to_string()).collect()
        }

        fn figures_once_running(pid: u32, program: &str) -> Option<ProcessFigures> {
            let mut figures = None;
            Deadline::for_background_work().wait_until(|| {
                figures = ProcfsProcessTable::figures(pid);
                figures
                    .as_ref()
                    .is_some_and(|figures| figures.program() == program)
            });
            figures
        }

        fn environment_once_holding(pid: u32, entry: &str) -> Vec<String> {
            let mut environment = Vec::new();
            Deadline::for_background_work().wait_until(|| {
                environment = ProcfsProcessTable::environment(pid);
                environment.iter().any(|variable| variable == entry)
            });
            environment
        }
    }

    #[test]
    fn a_server_of_tmux_is_named_after_its_program_and_role() {
        let words = TableFixture::words(&["/usr/bin/tmux", "-S", "/run/example.sock"]);

        assert_eq!(ProcfsProcessTable::server_name(&words), "tmux: server");
    }

    #[test]
    fn a_long_program_name_leaves_only_the_whole_words_that_fit() {
        let words = TableFixture::words(&["/opt/tmux-next", "-S", "/run/example.sock"]);

        assert_eq!(ProcfsProcessTable::server_name(&words), "tmux-next:");
    }

    #[test]
    fn the_pids_hold_this_process() {
        assert!(ProcfsProcessTable::pids().contains(&std::process::id()));
    }

    #[test]
    fn the_listed_pids_hold_this_process() {
        let pids = ProcfsProcessTable::listed_pids().unwrap();

        assert!(pids.contains(&std::process::id()));
    }

    #[test]
    fn a_child_is_measured_with_its_parent_and_program_from_its_stat_file() {
        let mut child = ProcessMarker::new().sleeping_child();

        let figures = TableFixture::figures_once_running(child.id(), "sleep");
        child.kill().unwrap();
        child.wait().unwrap();

        let figures = figures.unwrap();
        assert_eq!(figures.parent(), std::process::id());
        assert_eq!(figures.program(), "sleep");
    }

    #[test]
    fn a_running_process_holds_resident_memory() {
        let figures = ProcfsProcessTable::figures(std::process::id()).unwrap();

        assert!(figures.resident_memory() > MemorySize::default());
    }

    #[test]
    fn a_pid_that_no_process_has_has_no_figures() {
        assert_eq!(ProcfsProcessTable::figures(u32::MAX), None);
    }

    #[test]
    fn the_environment_of_a_child_holds_what_it_started_with() {
        let marker = ProcessMarker::new();
        let (name, value) = marker.variable();
        let entry = format!("{}={}", name.to_string_lossy(), value.to_string_lossy());
        let mut child = marker.sleeping_child();

        let environment = TableFixture::environment_once_holding(child.id(), &entry);
        child.kill().unwrap();
        child.wait().unwrap();

        assert!(environment.contains(&entry));
    }
}

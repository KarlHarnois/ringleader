use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::live_process::LiveProcess;

pub struct ProcfsProcessTable;

impl ProcfsProcessTable {
    const ROOT: &'static str = "/proc";
    const SERVER_TITLE: &'static str = ": server (";
    const PROCESS_NAME_LIMIT: usize = 15;

    pub fn pids() -> Vec<u32> {
        let Ok(entries) = fs::read_dir(Self::ROOT) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_string_lossy().parse().ok())
            .collect()
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
        let stat = fs::read_to_string(ProcfsProcessTable::directory_of(pid).join("stat")).ok()?;
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
        fn words(words: &[&str]) -> Vec<String> {
            words.iter().map(|word| word.to_string()).collect()
        }

        fn environment_once_started(pid: u32) -> Vec<String> {
            let mut environment = Vec::new();
            Deadline::for_background_work().wait_until(|| {
                environment = ProcfsProcessTable::environment(pid);
                !environment.is_empty()
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
    fn the_environment_of_a_child_holds_what_it_started_with() {
        let marker = ProcessMarker::new();
        let mut child = marker.sleeping_child();

        let environment = TableFixture::environment_once_started(child.id());
        child.kill().unwrap();
        child.wait().unwrap();

        let (name, value) = marker.variable();
        let entry = format!("{}={}", name.to_string_lossy(), value.to_string_lossy());
        assert!(environment.contains(&entry));
    }
}

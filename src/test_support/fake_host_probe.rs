use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use crate::application::ports::{HostProbe, HostProbeError, ProcessInfo};

#[derive(Default)]
pub struct FakeHostProbe {
    state: Mutex<HostState>,
}

#[derive(Default)]
struct HostState {
    commands: HashMap<String, PathBuf>,
    unwritable: HashSet<PathBuf>,
    directories: HashSet<PathBuf>,
    files: HashSet<PathBuf>,
    canonical: HashMap<PathBuf, PathBuf>,
    processes: Vec<FakeProcess>,
    terminated: Vec<u32>,
}

#[derive(Clone)]
struct FakeProcess {
    info: ProcessInfo,
    cwd: PathBuf,
    session: u32,
    survives_sigterm: bool,
}

impl FakeHostProbe {
    pub fn with_command(self, command: &str) -> Self {
        let resolved = PathBuf::from("/bin").join(command);
        self.state
            .lock()
            .unwrap()
            .commands
            .insert(command.to_string(), resolved);
        self
    }

    pub fn with_directory(self, path: &Path) -> Self {
        self.add_directory(path);
        self
    }

    pub fn with_file(self, path: &Path) -> Self {
        self.state.lock().unwrap().files.insert(path.to_path_buf());
        self
    }

    pub fn with_unwritable(self, path: &Path) -> Self {
        self.state
            .lock()
            .unwrap()
            .unwritable
            .insert(path.to_path_buf());
        self
    }

    pub fn with_canonical(self, path: &Path, canonical: &Path) -> Self {
        self.state
            .lock()
            .unwrap()
            .canonical
            .insert(path.to_path_buf(), canonical.to_path_buf());
        self
    }

    pub fn with_process(self, pid: u32, command: &str, cwd: &Path, session: u32) -> Self {
        self.add_process(pid, command, cwd, session, false);
        self
    }

    pub fn with_stubborn_process(self, pid: u32, command: &str, cwd: &Path, session: u32) -> Self {
        self.add_process(pid, command, cwd, session, true);
        self
    }

    pub fn add_directory(&self, path: &Path) {
        let mut state = self.state.lock().unwrap();
        for ancestor in path.ancestors() {
            state.directories.insert(ancestor.to_path_buf());
        }
    }

    pub fn remove_directory(&self, path: &Path) {
        let mut state = self.state.lock().unwrap();
        state
            .directories
            .retain(|directory| !directory.starts_with(path));
        state.files.retain(|file| !file.starts_with(path));
    }

    pub fn terminated(&self) -> Vec<u32> {
        self.state.lock().unwrap().terminated.clone()
    }

    fn add_process(&self, pid: u32, command: &str, cwd: &Path, session: u32, survives: bool) {
        self.state.lock().unwrap().processes.push(FakeProcess {
            info: ProcessInfo::new(pid, command),
            cwd: cwd.to_path_buf(),
            session,
            survives_sigterm: survives,
        });
    }

    fn has_parent_component(path: &Path) -> bool {
        path.components()
            .any(|component| component == Component::ParentDir)
    }
}

impl HostProbe for FakeHostProbe {
    fn resolve_command(&self, command: &str) -> Option<PathBuf> {
        self.state.lock().unwrap().commands.get(command).cloned()
    }

    fn is_writable(&self, path: &Path) -> bool {
        let state = self.state.lock().unwrap();
        state.directories.contains(path) && !state.unwritable.contains(path)
    }

    fn path_exists(&self, path: &Path) -> bool {
        let state = self.state.lock().unwrap();
        state.directories.contains(path) || state.files.contains(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.state.lock().unwrap().files.contains(path)
    }

    fn canonicalize_lenient(&self, path: &Path) -> Result<PathBuf, HostProbeError> {
        if !path.is_absolute() || Self::has_parent_component(path) {
            return Err(HostProbeError::RelativePath {
                path: path.to_path_buf(),
            });
        }
        let state = self.state.lock().unwrap();
        Ok(state
            .canonical
            .get(path)
            .cloned()
            .unwrap_or_else(|| path.to_path_buf()))
    }

    fn processes_in(&self, path: &Path, excluded_sessions: &[u32]) -> Vec<ProcessInfo> {
        let state = self.state.lock().unwrap();
        state
            .processes
            .iter()
            .filter(|process| process.cwd.starts_with(path))
            .filter(|process| !excluded_sessions.contains(&process.session))
            .map(|process| process.info.clone())
            .collect()
    }

    fn session_processes(&self, sessions: &[u32]) -> Vec<ProcessInfo> {
        let state = self.state.lock().unwrap();
        state
            .processes
            .iter()
            .filter(|process| sessions.contains(&process.session))
            .map(|process| process.info.clone())
            .collect()
    }

    fn terminate(&self, pids: &[u32]) -> Vec<ProcessInfo> {
        let mut state = self.state.lock().unwrap();
        state.terminated.extend_from_slice(pids);
        state
            .processes
            .retain(|process| process.survives_sigterm || !pids.contains(&process.info.pid()));
        state
            .processes
            .iter()
            .filter(|process| pids.contains(&process.info.pid()))
            .map(|process| process.info.clone())
            .collect()
    }
}

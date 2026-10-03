use std::path::Path;
use std::sync::{Condvar, Mutex};

use crate::application::ports::{RepoScanner, ScanError, ScanOutcome};
use crate::domain::{Repo, RepoScope};

pub struct FakeRepoScanner {
    repos: Vec<Repo>,
    enclosing_repo: Option<std::path::PathBuf>,
    scan_error: Option<ScanError>,
    cut_short: bool,
    gate: Gate,
}

impl FakeRepoScanner {
    pub fn finding(repos: Vec<Repo>) -> Self {
        Self {
            repos,
            enclosing_repo: None,
            scan_error: None,
            cut_short: false,
            gate: Gate::open(),
        }
    }

    pub fn inside_repo(self, top_level: &Path) -> Self {
        Self {
            enclosing_repo: Some(top_level.to_path_buf()),
            ..self
        }
    }

    pub fn failing(self, error: ScanError) -> Self {
        Self {
            scan_error: Some(error),
            ..self
        }
    }

    pub fn cut_short(self) -> Self {
        Self {
            cut_short: true,
            ..self
        }
    }

    pub fn blocked(self) -> Self {
        Self {
            gate: Gate::closed(),
            ..self
        }
    }

    pub fn release(&self) {
        self.gate.open_now();
    }
}

impl RepoScanner for FakeRepoScanner {
    fn resolve_scope(&self, launch_directory: &Path) -> Result<RepoScope, ScanError> {
        let scope = RepoScope::new(launch_directory);
        Ok(match &self.enclosing_repo {
            Some(top_level) => scope.inside_repo(top_level),
            None => scope,
        })
    }

    fn scan(&self, _scope: &RepoScope, _depth: usize) -> Result<ScanOutcome, ScanError> {
        self.gate.wait();
        match &self.scan_error {
            Some(error) => Err(error.clone()),
            None => Ok(ScanOutcome::new(self.repos.clone(), self.cut_short)),
        }
    }
}

struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn open() -> Self {
        Self {
            open: Mutex::new(true),
            changed: Condvar::new(),
        }
    }

    fn closed() -> Self {
        Self {
            open: Mutex::new(false),
            changed: Condvar::new(),
        }
    }

    fn open_now(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }

    fn wait(&self) {
        let open = self.open.lock().unwrap();
        drop(self.changed.wait_while(open, |open| !*open).unwrap());
    }
}

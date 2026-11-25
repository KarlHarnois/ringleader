use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::application::DeleteImpact;
use crate::application::ports::{DirtyState, ProcessInfo, UnmergedCommits};

#[derive(Debug, Serialize, Deserialize)]
pub struct ImpactJson {
    #[serde(default)]
    dirty_files: u64,
    #[serde(default)]
    unmerged_commits: u64,
    #[serde(default)]
    nested_repos: Vec<PathBuf>,
    #[serde(default)]
    processes: Vec<ProcessJson>,
    #[serde(default)]
    working: bool,
    #[serde(skip_deserializing)]
    warnings: Vec<String>,
}

impl ImpactJson {
    pub fn of(impact: &DeleteImpact) -> Self {
        Self::with_warnings(impact, impact.warnings())
    }

    pub fn with_warnings(impact: &DeleteImpact, warnings: Vec<String>) -> Self {
        Self {
            dirty_files: impact.dirty_files(),
            unmerged_commits: impact.unmerged_commits(),
            nested_repos: impact.nested_repos().to_vec(),
            processes: impact.processes().iter().map(ProcessJson::of).collect(),
            working: impact.working(),
            warnings,
        }
    }

    pub fn parse(text: &str) -> Result<DeleteImpact, ImpactJsonError> {
        let Self {
            dirty_files,
            unmerged_commits,
            nested_repos,
            processes,
            working,
            warnings: _,
        } = serde_json::from_str(text).map_err(ImpactJsonError)?;
        let dirty = DirtyState::present(dirty_files, nested_repos);
        let commits = UnmergedCommits::counting(unmerged_commits);
        Ok(
            DeleteImpact::measured(&dirty, &commits, Self::into_processes(processes))
                .while_working(working),
        )
    }

    fn into_processes(processes: Vec<ProcessJson>) -> Vec<ProcessInfo> {
        processes
            .into_iter()
            .map(ProcessJson::into_process_info)
            .collect()
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ProcessJson {
    pid: u32,
    command: String,
}

impl ProcessJson {
    fn of(process: &ProcessInfo) -> Self {
        Self {
            pid: process.pid(),
            command: process.command().to_string(),
        }
    }

    fn into_process_info(self) -> ProcessInfo {
        ProcessInfo::new(self.pid, &self.command)
    }
}

#[derive(Debug, Error)]
#[error("--if-impact: {0}")]
pub struct ImpactJsonError(#[source] serde_json::Error);

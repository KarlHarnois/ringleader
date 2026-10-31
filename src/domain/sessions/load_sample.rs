use std::cmp::Reverse;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::session_load::{ProcessLoad, SessionLoad};
use super::session_summary::SessionSummary;
use crate::domain::{CpuPercent, ProcessSample, RepoGroup};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadSample {
    processes: ProcessSample,
    sessions: Vec<SessionProcesses>,
}

impl LoadSample {
    pub const INTERVAL: Duration = Duration::from_secs(1);

    pub fn new(processes: ProcessSample) -> Self {
        Self {
            processes,
            sessions: Vec::new(),
        }
    }

    pub fn attributing(mut self, row: &SessionSummary, pane_pids: &[u32]) -> Self {
        let pids = self.processes.trees_from(pane_pids);
        self.sessions.push(SessionProcesses {
            name: row.name().to_string(),
            repo: RepoGroup::of(row).path().map(Path::to_path_buf),
            pids,
        });
        self
    }

    pub fn loads_since(&self, earlier: &LoadSample) -> Vec<SessionLoad> {
        let interval = SampleInterval {
            earlier: &earlier.processes,
            later: &self.processes,
        };
        let mut loads: Vec<SessionLoad> = self
            .sessions
            .iter()
            .map(|session| interval.session_load(session))
            .collect();
        loads.sort_by_key(|load| Reverse(load.busyness()));
        loads
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionProcesses {
    name: String,
    repo: Option<PathBuf>,
    pids: Vec<u32>,
}

struct SampleInterval<'a> {
    earlier: &'a ProcessSample,
    later: &'a ProcessSample,
}

impl SampleInterval<'_> {
    fn session_load(&self, session: &SessionProcesses) -> SessionLoad {
        let processes = session
            .pids
            .iter()
            .filter_map(|pid| self.process_load(*pid))
            .collect();
        SessionLoad::new(&session.name, processes).in_repo(session.repo.as_deref())
    }

    fn process_load(&self, pid: u32) -> Option<ProcessLoad> {
        let process = self.later.figures_of(pid)?;
        let spent = process.cpu_time_since(self.earlier.figures_of(pid));
        let elapsed = self.later.time_since(self.earlier);
        Some(ProcessLoad::new(process, CpuPercent::of(spent, elapsed)))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::domain::{ProcessFigures, SessionState};
    use crate::test_support::ServiceFixture;

    struct LoadSampleFixture;

    impl LoadSampleFixture {
        const PANE: u32 = 100;
        const AGENT: u32 = 101;
        const TOOL: u32 = 102;
        const STRANGER: u32 = 200;
        const OTHER_PANE: u32 = 300;

        fn process(pid: u32, parent: u32, cpu_milliseconds: u64) -> ProcessFigures {
            ProcessFigures::new(pid, parent, "example-program")
                .with_cpu_time(Duration::from_millis(cpu_milliseconds))
        }

        fn at(seconds: u64, processes: Vec<ProcessFigures>) -> ProcessSample {
            ProcessSample::new(Duration::from_secs(seconds), processes)
        }

        fn idle_baseline() -> ProcessSample {
            Self::at(
                0,
                [
                    Self::PANE,
                    Self::AGENT,
                    Self::TOOL,
                    Self::STRANGER,
                    Self::OTHER_PANE,
                ]
                .map(|pid| Self::process(pid, 1, 0))
                .to_vec(),
            )
        }

        fn row(name: &str) -> SessionSummary {
            SessionSummary::managed(ServiceFixture::session(name), SessionState::Working, false)
        }

        fn loads(later: ProcessSample, sessions: &[(&str, u32)]) -> Vec<SessionLoad> {
            let attributed = |sample: ProcessSample| {
                sessions
                    .iter()
                    .fold(LoadSample::new(sample), |sample, (name, pane)| {
                        sample.attributing(&Self::row(name), &[*pane])
                    })
            };
            attributed(later).loads_since(&attributed(Self::idle_baseline()))
        }

        fn pane_tree_beside_a_stranger(milliseconds_each: u64) -> ProcessSample {
            Self::at(
                1,
                vec![
                    Self::process(Self::PANE, 1, milliseconds_each),
                    Self::process(Self::AGENT, Self::PANE, milliseconds_each),
                    Self::process(Self::TOOL, Self::AGENT, milliseconds_each),
                    Self::process(Self::STRANGER, 1, 900),
                ],
            )
        }

        fn names(loads: &[SessionLoad]) -> Vec<&str> {
            loads.iter().map(SessionLoad::name).collect()
        }
    }

    #[test]
    fn a_session_counts_its_pane_process_and_every_descendant_of_it() {
        let later = LoadSampleFixture::pane_tree_beside_a_stranger(100);

        let loads = LoadSampleFixture::loads(later, &[("fix-auth", LoadSampleFixture::PANE)]);

        assert_eq!(loads[0].cpu().to_string(), "30.0%");
    }

    #[test]
    fn a_session_counts_no_process_outside_its_pane_trees() {
        let later = LoadSampleFixture::pane_tree_beside_a_stranger(0);

        let loads = LoadSampleFixture::loads(later, &[("fix-auth", LoadSampleFixture::PANE)]);

        assert_eq!(loads[0].cpu().to_string(), "0.0%");
    }

    #[test]
    fn the_cpu_of_a_process_is_the_time_it_spent_between_the_two_samples() {
        let earlier = LoadSampleFixture::at(
            0,
            vec![LoadSampleFixture::process(LoadSampleFixture::PANE, 1, 1000)],
        );
        let later = LoadSampleFixture::at(
            2,
            vec![LoadSampleFixture::process(LoadSampleFixture::PANE, 1, 1500)],
        );
        let row = LoadSampleFixture::row("fix-auth");
        let attributed =
            |sample| LoadSample::new(sample).attributing(&row, &[LoadSampleFixture::PANE]);

        let loads = attributed(later).loads_since(&attributed(earlier));

        assert_eq!(loads[0].cpu().to_string(), "25.0%");
    }

    #[test]
    fn sessions_are_sorted_by_cpu_highest_first() {
        let later = LoadSampleFixture::at(
            1,
            vec![
                LoadSampleFixture::process(LoadSampleFixture::PANE, 1, 100),
                LoadSampleFixture::process(LoadSampleFixture::OTHER_PANE, 1, 600),
            ],
        );

        let loads = LoadSampleFixture::loads(
            later,
            &[
                ("fix-auth", LoadSampleFixture::PANE),
                ("docs", LoadSampleFixture::OTHER_PANE),
            ],
        );

        assert_eq!(LoadSampleFixture::names(&loads), ["docs", "fix-auth"]);
    }

    #[test]
    fn a_session_load_names_the_repo_of_its_session() {
        let later = LoadSampleFixture::pane_tree_beside_a_stranger(0);

        let loads = LoadSampleFixture::loads(later, &[("fix-auth", LoadSampleFixture::PANE)]);

        assert_eq!(loads[0].repo(), Some(Path::new(ServiceFixture::REPO)));
    }

    #[test]
    fn a_session_whose_pane_process_is_gone_uses_nothing() {
        let later = LoadSampleFixture::at(1, Vec::new());

        let loads = LoadSampleFixture::loads(later, &[("fix-auth", LoadSampleFixture::PANE)]);

        assert!(loads[0].busiest().is_empty());
    }
}

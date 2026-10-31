use std::sync::Arc;

use super::failure_streaks::FailureStreaks;
use super::pending::Pending;
use super::ports::{Multiplexer, ProcessMeter, ProcessMeterError};
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use crate::domain::{LoadSample, ProcessSample, SessionState, SessionSummary};

#[derive(Clone)]
pub struct LoadMonitor {
    multiplexer: Arc<dyn Multiplexer>,
    meter: Arc<dyn ProcessMeter>,
    failures: Arc<FailureStreaks>,
}

impl LoadMonitor {
    const PROCESS_TABLE_STREAK: &'static str = "the process table";

    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            multiplexer: ports.tmux().multiplexer(),
            meter: ports.agents().process_meter(),
            failures: Arc::default(),
        }
    }

    pub fn sample(&self, rows: &[SessionSummary]) -> Pending<LoadSample> {
        let monitor = self.clone();
        let running: Vec<RunningSession> = rows
            .iter()
            .filter(|row| row.state() != SessionState::Stopped)
            .map(RunningSession::of)
            .collect();
        Pending::start(move || monitor.sample_now(&running))
    }

    fn sample_now(&self, sessions: &[RunningSession]) -> Result<LoadSample, ServiceError> {
        let panes: Vec<(&RunningSession, Vec<u32>)> = sessions
            .iter()
            .filter_map(|session| Some((session, self.pane_pids(session)?)))
            .collect();
        let processes = self.processes()?;
        Ok(panes.iter().fold(
            LoadSample::new(processes),
            |sample, (session, pane_pids)| sample.attributing(&session.name, pane_pids),
        ))
    }

    fn pane_pids(&self, session: &RunningSession) -> Option<Vec<u32>> {
        let listed = self.multiplexer.pane_pids(&session.tmux_name);
        match &listed {
            Ok(_) => self.failures.end(&session.name),
            Err(error) => self.failures.warn_once(
                &session.name,
                &format!(
                    "left {} out of the resource view, its panes could not be listed: {error}",
                    session.name
                ),
            ),
        }
        listed.ok()
    }

    fn processes(&self) -> Result<ProcessSample, ProcessMeterError> {
        let sampled = self.meter.sample();
        match &sampled {
            Ok(_) => self.failures.end(Self::PROCESS_TABLE_STREAK),
            Err(error) => self
                .failures
                .warn_once(Self::PROCESS_TABLE_STREAK, &error.to_string()),
        }
        sampled
    }
}

struct RunningSession {
    name: String,
    tmux_name: String,
}

impl RunningSession {
    fn of(row: &SessionSummary) -> Self {
        Self {
            name: row.name().to_string(),
            tmux_name: row.tmux_name().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::application::ports::MultiplexerError;
    use crate::domain::{ProcessFigures, SessionLoad};
    use crate::test_support::{FakeMultiplexer, FakeProcessMeter, LogCapture, ServiceFixture};

    struct MonitorFixture;

    impl MonitorFixture {
        const PANE: u32 = 100;
        const CHILD: u32 = 101;
        const STRANGER: u32 = 200;
        const OTHER_PANE: u32 = 300;

        fn process(pid: u32, parent: u32, cpu_milliseconds: u64) -> ProcessFigures {
            ProcessFigures::new(pid, parent, "example-program")
                .with_cpu_time(Duration::from_millis(cpu_milliseconds))
        }

        fn at(seconds: u64, processes: Vec<ProcessFigures>) -> ProcessSample {
            ProcessSample::new(Duration::from_secs(seconds), processes)
        }

        fn idle_then(later: Vec<ProcessFigures>) -> FakeProcessMeter {
            let idle = [Self::PANE, Self::CHILD, Self::STRANGER, Self::OTHER_PANE]
                .map(|pid| Self::process(pid, 1, 0))
                .to_vec();
            FakeProcessMeter::new().with_samples(vec![Self::at(0, idle), Self::at(1, later)])
        }

        fn row(name: &str, state: SessionState) -> SessionSummary {
            SessionSummary::managed(ServiceFixture::session(name), state, false)
        }

        fn tmux_name(name: &str) -> String {
            ServiceFixture::session(name).tmux_name().to_string()
        }

        fn monitor(multiplexer: FakeMultiplexer, meter: FakeProcessMeter) -> LoadMonitor {
            let fixture = ServiceFixture::new()
                .with_multiplexer(multiplexer)
                .with_process_meter(meter);
            LoadMonitor::new(&fixture.ports())
        }

        fn sampled(
            monitor: &LoadMonitor,
            rows: &[SessionSummary],
        ) -> Result<LoadSample, ServiceError> {
            monitor.sample(rows).wait_unless(|| false).unwrap()
        }

        fn loads(monitor: &LoadMonitor, rows: &[SessionSummary]) -> Vec<SessionLoad> {
            let earlier = Self::sampled(monitor, rows).unwrap();
            Self::sampled(monitor, rows).unwrap().loads_since(&earlier)
        }

        fn names(loads: &[SessionLoad]) -> Vec<&str> {
            loads.iter().map(SessionLoad::name).collect()
        }
    }

    #[test]
    fn a_running_session_is_given_the_load_of_its_pane_trees() {
        let multiplexer = FakeMultiplexer::new().with_pane_pids(
            &MonitorFixture::tmux_name("fix-auth"),
            &[MonitorFixture::PANE],
        );
        let meter = MonitorFixture::idle_then(vec![
            MonitorFixture::process(MonitorFixture::PANE, 1, 100),
            MonitorFixture::process(MonitorFixture::CHILD, MonitorFixture::PANE, 200),
            MonitorFixture::process(MonitorFixture::STRANGER, 1, 900),
        ]);
        let monitor = MonitorFixture::monitor(multiplexer, meter);

        let loads = MonitorFixture::loads(
            &monitor,
            &[MonitorFixture::row("fix-auth", SessionState::Working)],
        );

        assert_eq!(loads[0].cpu().to_string(), "30.0%");
    }

    #[test]
    fn a_stopped_session_is_not_sampled() {
        let monitor = MonitorFixture::monitor(FakeMultiplexer::new(), FakeProcessMeter::new());

        let loads = MonitorFixture::loads(
            &monitor,
            &[
                MonitorFixture::row("fix-auth", SessionState::Waiting),
                MonitorFixture::row("docs", SessionState::Stopped),
            ],
        );

        assert_eq!(MonitorFixture::names(&loads), ["fix-auth"]);
    }

    #[test]
    fn a_session_whose_panes_cannot_be_listed_is_left_out() {
        let failure = MultiplexerError::Failed {
            subcommand: "list-panes".to_string(),
            detail: "can't find session".to_string(),
        };
        let multiplexer = FakeMultiplexer::new().failing("pane_pids", failure);
        let monitor = MonitorFixture::monitor(multiplexer, FakeProcessMeter::new());

        let loads = MonitorFixture::loads(
            &monitor,
            &[MonitorFixture::row("fix-auth", SessionState::Waiting)],
        );

        assert!(loads.is_empty());
    }

    #[test]
    fn an_unreadable_process_table_fails_the_sample() {
        let meter = FakeProcessMeter::new().unreadable("example denial of one sample");
        let monitor = MonitorFixture::monitor(FakeMultiplexer::new(), meter);

        let sampled = MonitorFixture::sampled(&monitor, &[]);

        assert!(sampled.is_err());
    }

    #[test]
    fn a_process_table_that_stays_unreadable_is_warned_about_once() {
        let log = LogCapture::installed();
        let meter = FakeProcessMeter::new().unreadable("example denial that lasts");
        let monitor = MonitorFixture::monitor(FakeMultiplexer::new(), meter);

        for _ in 0..3 {
            let _ = MonitorFixture::sampled(&monitor, &[]);
        }

        assert_eq!(log.count_containing("example denial that lasts"), 1);
    }

    #[test]
    fn each_session_is_given_only_its_own_panes() {
        let multiplexer = FakeMultiplexer::new()
            .with_pane_pids(
                &MonitorFixture::tmux_name("fix-auth"),
                &[MonitorFixture::PANE],
            )
            .with_pane_pids(
                &MonitorFixture::tmux_name("docs"),
                &[MonitorFixture::OTHER_PANE],
            );
        let meter = MonitorFixture::idle_then(vec![
            MonitorFixture::process(MonitorFixture::PANE, 1, 100),
            MonitorFixture::process(MonitorFixture::OTHER_PANE, 1, 700),
        ]);
        let monitor = MonitorFixture::monitor(multiplexer, meter);

        let loads = MonitorFixture::loads(
            &monitor,
            &[
                MonitorFixture::row("fix-auth", SessionState::Waiting),
                MonitorFixture::row("docs", SessionState::Working),
            ],
        );

        assert_eq!(loads[0].name(), "docs");
        assert_eq!(loads[0].cpu().to_string(), "70.0%");
    }
}

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use super::failure_streaks::FailureStreaks;
use super::pending::Pending;
use super::ports::{Clock, UsageError, UsageMeter};
use super::usage_reading::{AgentReading, UsageReading};
use crate::domain::{Agent, AgentUsage};

#[derive(Clone)]
pub struct UsageCatalog {
    meters: Vec<Arc<dyn UsageMeter>>,
    clock: Arc<dyn Clock>,
    latest: Arc<Mutex<Option<UsageReading>>>,
    measuring: Arc<Mutex<()>>,
    completed_measurements: Arc<AtomicU64>,
    failures: Arc<FailureStreaks>,
}

impl UsageCatalog {
    pub fn new(meters: Vec<Arc<dyn UsageMeter>>, clock: Arc<dyn Clock>) -> Self {
        Self {
            meters,
            clock,
            latest: Arc::default(),
            measuring: Arc::default(),
            completed_measurements: Arc::default(),
            failures: Arc::default(),
        }
    }

    pub fn measure(&self, agents: &[Agent]) -> Pending<UsageReading> {
        let catalog = self.clone();
        let agents = agents.to_vec();
        let completed_before = self.completed_measurements.load(Ordering::SeqCst);
        Pending::start(move || Ok(catalog.record(completed_before, &agents)))
    }

    pub fn latest(&self) -> Option<UsageReading> {
        self.stored().clone()
    }

    fn record(&self, completed_before: u64, agents: &[Agent]) -> UsageReading {
        let _measuring = self
            .measuring
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(reading) = self.reading_completed_since(completed_before) {
            return reading;
        }
        let readings = self.readings_at_once(agents);
        let reading = UsageReading::new(self.clock.now_epoch_seconds(), readings);
        *self.stored() = Some(reading.clone());
        self.completed_measurements.fetch_add(1, Ordering::SeqCst);
        reading
    }

    fn reading_completed_since(&self, completed_before: u64) -> Option<UsageReading> {
        let completed_since = self.completed_measurements.load(Ordering::SeqCst) > completed_before;
        self.latest().filter(|_| completed_since)
    }

    fn readings_at_once(&self, agents: &[Agent]) -> Vec<AgentReading> {
        thread::scope(|scope| {
            let measurements: Vec<_> = agents
                .iter()
                .map(|agent| (agent, scope.spawn(|| self.reading_of(agent))))
                .collect();
            measurements
                .into_iter()
                .filter_map(|(agent, measurement)| {
                    measurement.join().unwrap_or_else(|_| {
                        log::warn!(
                            "the usage measurement of the agent {} panicked",
                            agent.name()
                        );
                        None
                    })
                })
                .collect()
        })
    }

    fn reading_of(&self, agent: &Agent) -> Option<AgentReading> {
        let usage = self.measured_by_first_claiming_meter(agent).transpose()?;
        self.track_failure(agent.name(), &usage);
        Some(AgentReading::new(agent.name(), usage))
    }

    fn measured_by_first_claiming_meter(
        &self,
        agent: &Agent,
    ) -> Result<Option<AgentUsage>, UsageError> {
        for meter in &self.meters {
            if let Some(usage) = meter.measure(agent)? {
                return Ok(Some(usage));
            }
        }
        Ok(None)
    }

    fn track_failure(&self, agent: &str, usage: &Result<AgentUsage, UsageError>) {
        match usage {
            Ok(_) => self.failures.end(agent),
            Err(error) => self.failures.warn_once(
                agent,
                &format!("could not measure the usage of the agent {agent}: {error}"),
            ),
        }
    }

    fn stored(&self) -> MutexGuard<'_, Option<UsageReading>> {
        self.latest.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{LimitWindow, UsageLimit, UsageRow};
    use crate::test_support::{FakeClock, FakeUsageMeter, HeldCalls, LogCapture, ServiceFixture};

    struct CatalogFixture;

    impl CatalogFixture {
        fn agent(name: &str) -> Agent {
            Agent::new(name, name)
        }

        fn usage(percent: f64) -> AgentUsage {
            let limit = UsageLimit::used(LimitWindow::Session, percent, None);
            AgentUsage::new(vec![UsageRow::Limit(limit)])
        }

        fn catalog(meters: Vec<Arc<dyn UsageMeter>>) -> UsageCatalog {
            UsageCatalog::new(meters, Arc::new(FakeClock::at(ServiceFixture::NOW)))
        }

        fn measured(catalog: &UsageCatalog, agents: &[Agent]) -> UsageReading {
            catalog
                .measure(agents)
                .wait_unless(|| false)
                .unwrap()
                .unwrap()
        }

        fn names(reading: &UsageReading) -> Vec<&str> {
            reading.agents().iter().map(AgentReading::agent).collect()
        }
    }

    #[test]
    fn an_agent_no_meter_claims_has_no_reading() {
        let catalog = CatalogFixture::catalog(vec![Arc::new(FakeUsageMeter::new())]);

        let reading = CatalogFixture::measured(&catalog, &[CatalogFixture::agent("shell")]);

        assert!(reading.agents().is_empty());
    }

    #[test]
    fn readings_follow_the_config_order() {
        let meter = FakeUsageMeter::new()
            .with_usage("work", CatalogFixture::usage(10.0))
            .with_usage("home", CatalogFixture::usage(20.0));
        let catalog = CatalogFixture::catalog(vec![Arc::new(meter)]);
        let agents = [CatalogFixture::agent("work"), CatalogFixture::agent("home")];

        let reading = CatalogFixture::measured(&catalog, &agents);

        assert_eq!(CatalogFixture::names(&reading), ["work", "home"]);
    }

    #[test]
    fn every_agent_is_measured_at_the_same_time() {
        let held = HeldCalls::all();
        let meter = FakeUsageMeter::new();
        meter.on_measure(held.hook());
        let catalog = CatalogFixture::catalog(vec![Arc::new(meter)]);
        let agents = [CatalogFixture::agent("work"), CatalogFixture::agent("home")];

        let measurement = catalog.measure(&agents);
        let both_started = held.wait_for(2);
        held.release();
        measurement.wait_unless(|| false);

        assert!(both_started);
    }

    #[test]
    fn a_measurement_asked_for_during_another_reuses_its_reading() {
        let held = HeldCalls::all();
        let meter = Arc::new(FakeUsageMeter::new());
        meter.on_measure(held.hook());
        let catalog = CatalogFixture::catalog(vec![meter.clone()]);
        let agents = [CatalogFixture::agent("work")];

        let first = catalog.measure(&agents);
        held.wait_for(1);
        let second = catalog.measure(&agents);
        held.release();

        assert_eq!(second.wait_unless(|| false), first.wait_unless(|| false));
        assert_eq!(meter.calls().len(), 1);
    }

    #[test]
    fn a_measurement_asked_for_after_the_last_reading_measures_again() {
        let meter = Arc::new(FakeUsageMeter::new());
        let catalog = CatalogFixture::catalog(vec![meter.clone()]);
        let agents = [CatalogFixture::agent("work")];

        CatalogFixture::measured(&catalog, &agents);
        CatalogFixture::measured(&catalog, &agents);

        assert_eq!(meter.calls().len(), 2);
    }

    #[test]
    fn the_first_meter_that_claims_the_agent_measures_it() {
        let unclaiming = Arc::new(FakeUsageMeter::new());
        let claiming = FakeUsageMeter::new().with_usage("work", CatalogFixture::usage(10.0));
        let later = FakeUsageMeter::new().with_usage("work", CatalogFixture::usage(90.0));
        let catalog =
            CatalogFixture::catalog(vec![unclaiming, Arc::new(claiming), Arc::new(later)]);

        let reading = CatalogFixture::measured(&catalog, &[CatalogFixture::agent("work")]);

        assert_eq!(
            reading.agents()[0].usage(),
            Ok(&CatalogFixture::usage(10.0))
        );
    }

    #[test]
    fn a_meter_that_fails_reports_the_failure_instead_of_asking_the_next_meter() {
        let failing = FakeUsageMeter::new();
        failing.fail("work");
        let later = FakeUsageMeter::new().with_usage("work", CatalogFixture::usage(90.0));
        let catalog = CatalogFixture::catalog(vec![Arc::new(failing), Arc::new(later)]);

        let reading = CatalogFixture::measured(&catalog, &[CatalogFixture::agent("work")]);

        assert_eq!(reading.agents()[0].usage(), Err(&UsageError::Unanswered));
    }

    #[test]
    fn a_failing_agent_keeps_the_others_measured() {
        let meter = FakeUsageMeter::new().with_usage("home", CatalogFixture::usage(20.0));
        meter.fail("work");
        let catalog = CatalogFixture::catalog(vec![Arc::new(meter)]);
        let agents = [CatalogFixture::agent("work"), CatalogFixture::agent("home")];

        let reading = CatalogFixture::measured(&catalog, &agents);

        assert_eq!(
            reading.agents()[1].usage(),
            Ok(&CatalogFixture::usage(20.0))
        );
    }

    #[test]
    fn a_reading_is_stamped_with_the_clock() {
        let catalog = CatalogFixture::catalog(Vec::new());

        let reading = CatalogFixture::measured(&catalog, &[]);

        assert_eq!(reading.measured_at(), ServiceFixture::NOW);
    }

    #[test]
    fn the_latest_reading_outlives_its_pending() {
        let meter = FakeUsageMeter::new().with_usage("work", CatalogFixture::usage(10.0));
        let catalog = CatalogFixture::catalog(vec![Arc::new(meter)]);

        let reading = CatalogFixture::measured(&catalog, &[CatalogFixture::agent("work")]);

        assert_eq!(catalog.latest(), Some(reading));
    }

    #[test]
    fn nothing_is_known_before_the_first_measurement() {
        assert_eq!(CatalogFixture::catalog(Vec::new()).latest(), None);
    }

    #[test]
    fn an_agent_that_keeps_failing_is_warned_about_once() {
        let log = LogCapture::installed();
        let meter = FakeUsageMeter::new();
        meter.fail("always-failing-agent");
        let catalog = CatalogFixture::catalog(vec![Arc::new(meter)]);
        let agents = [CatalogFixture::agent("always-failing-agent")];

        for _ in 0..3 {
            CatalogFixture::measured(&catalog, &agents);
        }

        assert_eq!(log.count_containing("always-failing-agent"), 1);
    }

    #[test]
    fn an_agent_that_fails_again_after_recovering_is_warned_about_again() {
        let log = LogCapture::installed();
        let meter = Arc::new(
            FakeUsageMeter::new().with_usage("failing-again-agent", CatalogFixture::usage(10.0)),
        );
        let catalog = CatalogFixture::catalog(vec![meter.clone()]);
        let agents = [CatalogFixture::agent("failing-again-agent")];

        meter.fail("failing-again-agent");
        CatalogFixture::measured(&catalog, &agents);
        meter.recover("failing-again-agent");
        CatalogFixture::measured(&catalog, &agents);
        meter.fail("failing-again-agent");
        CatalogFixture::measured(&catalog, &agents);

        assert_eq!(log.count_containing("failing-again-agent"), 2);
    }
}

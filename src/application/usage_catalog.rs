use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use super::pending::Pending;
use super::ports::{Clock, UsageError, UsageMeter};
use super::usage_reading::{AgentReading, UsageReading};
use crate::domain::{Agent, AgentUsage};

#[derive(Clone)]
pub struct UsageCatalog {
    meters: Vec<Arc<dyn UsageMeter>>,
    clock: Arc<dyn Clock>,
    latest: Arc<Mutex<Option<UsageReading>>>,
    failing_agents: Arc<Mutex<HashSet<String>>>,
}

impl UsageCatalog {
    pub fn new(meters: Vec<Arc<dyn UsageMeter>>, clock: Arc<dyn Clock>) -> Self {
        Self {
            meters,
            clock,
            latest: Arc::default(),
            failing_agents: Arc::default(),
        }
    }

    pub fn measure(&self, agents: &[Agent]) -> Pending<UsageReading> {
        let catalog = self.clone();
        let agents = agents.to_vec();
        Pending::start(move || Ok(catalog.record(&agents)))
    }

    pub fn latest(&self) -> Option<UsageReading> {
        self.stored().clone()
    }

    fn record(&self, agents: &[Agent]) -> UsageReading {
        let readings = self.readings_at_once(agents);
        let reading = UsageReading::new(self.clock.now_epoch_seconds(), readings);
        *self.stored() = Some(reading.clone());
        reading
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
            Ok(_) => {
                self.failing_agents().remove(agent);
            }
            Err(error) => self.warn_once(agent, error),
        }
    }

    fn warn_once(&self, agent: &str, error: &UsageError) {
        if self.failing_agents().insert(agent.to_string()) {
            log::warn!("could not measure the usage of the agent {agent}: {error}");
        }
    }

    fn stored(&self) -> MutexGuard<'_, Option<UsageReading>> {
        self.latest.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn failing_agents(&self) -> MutexGuard<'_, HashSet<String>> {
        self.failing_agents
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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

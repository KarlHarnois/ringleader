use std::sync::Arc;

use super::opencode_activity::OpencodeActivity;
use super::provider_quota::ProviderQuota;
use crate::adapters::agent_launch::agent_probe::AgentProbe;
use crate::application::ports::{UsageError, UsageMeter};
use crate::domain::{Agent, AgentUsage, UsageRow};

pub struct OpencodeUsageMeter {
    probe: Arc<AgentProbe>,
    quotas: Vec<Arc<dyn ProviderQuota>>,
}

impl OpencodeUsageMeter {
    const PROGRAM: &'static str = "opencode";
    const DATABASE_ARGUMENTS: [&'static str; 5] =
        ["--pure", "db", "--format", "json", OpencodeActivity::QUERY];

    pub fn new(probe: Arc<AgentProbe>, quotas: Vec<Arc<dyn ProviderQuota>>) -> Self {
        Self { probe, quotas }
    }

    fn usage(&self, activity: &OpencodeActivity) -> AgentUsage {
        match activity
            .provider()
            .and_then(|provider| self.quota(provider))
        {
            Some(rows) => activity.usage_with(rows),
            None => activity.usage(),
        }
    }

    fn quota(&self, provider: &str) -> Option<Vec<UsageRow>> {
        for quota in &self.quotas {
            match quota.measure(provider) {
                Ok(Some(rows)) => return Some(rows),
                Ok(None) => {}
                Err(error) => {
                    log::warn!(
                        "could not read the {provider} quota, showing opencode's totals: {error}"
                    );
                    return None;
                }
            }
        }
        None
    }
}

impl UsageMeter for OpencodeUsageMeter {
    fn measure(&self, agent: &Agent) -> Result<Option<AgentUsage>, UsageError> {
        if agent.command_name() != Self::PROGRAM {
            return Ok(None);
        }
        let output = self.probe.run(agent, &Self::DATABASE_ARGUMENTS)?;
        let activity = OpencodeActivity::parse(&output)?;
        Ok(Some(
            activity
                .map(|activity| self.usage(&activity))
                .unwrap_or_default(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use super::*;
    use crate::adapters::host::process_environment::ProcessEnvironment;
    use crate::adapters::opencode::provider_quota::QuotaError;
    use crate::adapters::{BoundedCommand, Deadlines};
    use crate::domain::{LimitWindow, UsageLimit};
    use crate::test_support::{ExecutableScript, InheritedPath};

    struct MeterFixture {
        directory: tempfile::TempDir,
    }

    struct FakeProviderQuota {
        answer: Result<Option<Vec<UsageRow>>, QuotaError>,
        asked: Mutex<Vec<String>>,
    }

    impl FakeProviderQuota {
        fn answering(answer: Result<Option<Vec<UsageRow>>, QuotaError>) -> Arc<Self> {
            Arc::new(Self {
                answer,
                asked: Mutex::default(),
            })
        }

        fn asked(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }
    }

    impl ProviderQuota for FakeProviderQuota {
        fn measure(&self, provider: &str) -> Result<Option<Vec<UsageRow>>, QuotaError> {
            self.asked.lock().unwrap().push(provider.to_string());
            self.answer.clone()
        }
    }

    impl MeterFixture {
        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn agent_named(&self, program: &str, output: &str) -> Agent {
            let path = self.directory.path().join(program);
            let body = format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\ncat '{output}'\n");
            ExecutableScript::write(&path, &body);
            Agent::new("work", &path.to_string_lossy())
        }

        fn recording(&self, program: &str) -> Agent {
            let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/opencode-db-activity.json");
            self.agent_named(program, &fixture.to_string_lossy())
        }

        fn without_sessions(&self) -> Agent {
            let empty = self.directory.path().join("empty.json");
            fs::write(&empty, "[]").unwrap();
            self.agent_named("opencode", &empty.to_string_lossy())
        }

        fn meter(&self) -> OpencodeUsageMeter {
            self.meter_with(Vec::new())
        }

        fn meter_with(&self, quotas: Vec<Arc<dyn ProviderQuota>>) -> OpencodeUsageMeter {
            let variables = vec![(OsString::from("PATH"), InheritedPath::value())];
            let probe = Arc::new(AgentProbe::new(
                &ProcessEnvironment::new(variables, None),
                &self.directory.path().join("usage"),
                BoundedCommand::new(Deadlines::standard()),
            ));
            OpencodeUsageMeter::new(probe, quotas)
        }

        fn measured_with(&self, quotas: Vec<Arc<dyn ProviderQuota>>) -> AgentUsage {
            self.meter_with(quotas)
                .measure(&self.recording("opencode"))
                .unwrap()
                .unwrap()
        }

        fn credits() -> Vec<UsageRow> {
            let limit = UsageLimit::used(LimitWindow::Credits, 31.0, None);
            vec![UsageRow::Limit(limit)]
        }

        fn recorded_arguments(&self, program: &str) -> Option<Vec<String>> {
            let path = self.directory.path().join(format!("{program}.args"));
            let arguments = fs::read_to_string(path).ok()?;
            Some(arguments.lines().map(str::to_string).collect())
        }
    }

    #[test]
    fn an_agent_whose_command_is_not_opencode_is_left_to_another_meter() {
        let fixture = MeterFixture::new();
        let agent = fixture.recording("claude");

        assert_eq!(fixture.meter().measure(&agent), Ok(None));
        assert_eq!(fixture.recorded_arguments("claude"), None);
    }

    #[test]
    fn the_query_reads_the_database_without_plugins() {
        let fixture = MeterFixture::new();

        fixture
            .meter()
            .measure(&fixture.recording("opencode"))
            .unwrap();

        let arguments = fixture.recorded_arguments("opencode").unwrap();
        assert_eq!(arguments[..4], ["--pure", "db", "--format", "json"]);
        assert!(arguments[4].starts_with("with active as"));
    }

    #[test]
    fn the_activity_becomes_the_agents_usage() {
        let fixture = MeterFixture::new();

        let usage = fixture
            .meter()
            .measure(&fixture.recording("opencode"))
            .unwrap()
            .unwrap();

        assert_eq!(usage.model(), Some("example-provider/example-model"));
        assert_eq!(usage.rows().len(), 2);
    }

    #[test]
    fn the_active_providers_quota_replaces_the_totals() {
        let quota = FakeProviderQuota::answering(Ok(Some(MeterFixture::credits())));

        let usage = MeterFixture::new().measured_with(vec![quota]);

        assert_eq!(usage.rows(), MeterFixture::credits());
        assert_eq!(usage.model(), Some("example-provider/example-model"));
    }

    #[test]
    fn the_quota_is_asked_about_the_active_provider() {
        let quota = FakeProviderQuota::answering(Ok(None));

        MeterFixture::new().measured_with(vec![quota.clone()]);

        assert_eq!(quota.asked(), ["example-provider"]);
    }

    #[test]
    fn the_first_quota_that_knows_the_provider_answers() {
        let unknowing = FakeProviderQuota::answering(Ok(None));
        let knowing = FakeProviderQuota::answering(Ok(Some(MeterFixture::credits())));

        let usage = MeterFixture::new().measured_with(vec![unknowing, knowing]);

        assert_eq!(usage.rows(), MeterFixture::credits());
    }

    #[test]
    fn a_failing_quota_falls_back_to_the_totals() {
        let failing = FakeProviderQuota::answering(Err(QuotaError::Throttled));
        let later = FakeProviderQuota::answering(Ok(Some(MeterFixture::credits())));

        let usage = MeterFixture::new().measured_with(vec![failing, later.clone()]);

        assert_eq!(usage.rows().len(), 2);
        assert!(later.asked().is_empty());
    }

    #[test]
    fn an_opencode_without_sessions_reports_no_usage() {
        let fixture = MeterFixture::new();

        let usage = fixture
            .meter()
            .measure(&fixture.without_sessions())
            .unwrap();

        assert_eq!(usage, Some(AgentUsage::default()));
    }
}

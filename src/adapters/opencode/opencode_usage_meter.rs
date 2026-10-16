use std::sync::Arc;

use super::opencode_activity::OpencodeActivity;
use crate::adapters::agent_launch::agent_probe::AgentProbe;
use crate::application::ports::{UsageError, UsageMeter};
use crate::domain::{Agent, AgentUsage};

pub struct OpencodeUsageMeter {
    probe: Arc<AgentProbe>,
}

impl OpencodeUsageMeter {
    const PROGRAM: &'static str = "opencode";
    const DATABASE_ARGUMENTS: [&'static str; 5] =
        ["--pure", "db", "--format", "json", OpencodeActivity::QUERY];

    pub fn new(probe: Arc<AgentProbe>) -> Self {
        Self { probe }
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
                .map(|activity| activity.usage())
                .unwrap_or_default(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::adapters::host::process_environment::ProcessEnvironment;
    use crate::adapters::{BoundedCommand, Deadlines};
    use crate::test_support::{ExecutableScript, InheritedPath};

    struct MeterFixture {
        directory: tempfile::TempDir,
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
            let variables = vec![(OsString::from("PATH"), InheritedPath::value())];
            OpencodeUsageMeter::new(Arc::new(AgentProbe::new(
                &ProcessEnvironment::new(variables, None),
                &self.directory.path().join("usage"),
                BoundedCommand::new(Deadlines::standard()),
            )))
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
    fn an_opencode_without_sessions_reports_no_usage() {
        let fixture = MeterFixture::new();

        let usage = fixture
            .meter()
            .measure(&fixture.without_sessions())
            .unwrap();

        assert_eq!(usage, Some(AgentUsage::default()));
    }
}

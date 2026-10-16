use std::sync::Arc;

use super::claude_usage_report::ClaudeUsageReport;
use crate::adapters::agent_launch::agent_probe::AgentProbe;
use crate::application::ports::{UsageError, UsageMeter};
use crate::domain::{Agent, AgentUsage};

pub struct ClaudeUsageMeter {
    probe: Arc<AgentProbe>,
}

impl ClaudeUsageMeter {
    const PROGRAM: &'static str = "claude";
    const USAGE_ARGUMENTS: [&'static str; 9] = [
        "-p",
        "/usage",
        "--output-format",
        "stream-json",
        "--verbose",
        "--no-session-persistence",
        "--strict-mcp-config",
        "--settings",
        r#"{"disableAllHooks":true}"#,
    ];

    pub fn new(probe: Arc<AgentProbe>) -> Self {
        Self { probe }
    }
}

impl UsageMeter for ClaudeUsageMeter {
    fn measure(&self, agent: &Agent) -> Result<Option<AgentUsage>, UsageError> {
        if agent.command_name() != Self::PROGRAM {
            return Ok(None);
        }
        let stream = self.probe.run(agent, &Self::USAGE_ARGUMENTS)?;
        ClaudeUsageReport::parse(&stream).map(Some)
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

        fn agent_named(&self, program: &str) -> Agent {
            let path = self.directory.path().join(program);
            let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/claude-usage-stream.jsonl");
            let body = format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\ncat '{}'\n",
                fixture.display()
            );
            ExecutableScript::write(&path, &body);
            Agent::new("work", &path.to_string_lossy())
        }

        fn meter(&self) -> ClaudeUsageMeter {
            let variables = vec![(OsString::from("PATH"), InheritedPath::value())];
            ClaudeUsageMeter::new(Arc::new(AgentProbe::new(
                &ProcessEnvironment::new(variables, None),
                &self.directory.path().join("usage"),
                BoundedCommand::new(Deadlines::standard()),
            )))
        }

        fn recorded_arguments(&self, program: &str) -> Option<String> {
            let path = self.directory.path().join(format!("{program}.args"));
            fs::read_to_string(path).ok()
        }
    }

    #[test]
    fn an_agent_whose_command_is_not_claude_is_left_to_another_meter() {
        let fixture = MeterFixture::new();
        let agent = fixture.agent_named("opencode");

        assert_eq!(fixture.meter().measure(&agent), Ok(None));
        assert_eq!(fixture.recorded_arguments("opencode"), None);
    }

    #[test]
    fn the_probe_asks_for_the_report_without_saving_a_session_or_running_hooks_or_mcp_servers() {
        let fixture = MeterFixture::new();

        fixture
            .meter()
            .measure(&fixture.agent_named("claude"))
            .unwrap();

        assert_eq!(
            fixture.recorded_arguments("claude").unwrap(),
            "-p\n/usage\n--output-format\nstream-json\n--verbose\n--no-session-persistence\n\
             --strict-mcp-config\n--settings\n{\"disableAllHooks\":true}\n"
        );
    }

    #[test]
    fn the_report_becomes_the_agents_usage() {
        let fixture = MeterFixture::new();

        let usage = fixture
            .meter()
            .measure(&fixture.agent_named("claude"))
            .unwrap()
            .unwrap();

        assert_eq!(usage.rows().len(), 4);
    }
}

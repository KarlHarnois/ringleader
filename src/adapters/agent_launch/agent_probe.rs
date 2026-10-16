use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::agent_variables::AgentVariables;
use crate::adapters::BoundedCommand;
use crate::adapters::child_process::bounded_command::{Call, CommandError};
use crate::adapters::child_process::command_output::CommandOutput;
use crate::adapters::child_process::deadlines::CallKind;
use crate::adapters::host::process_environment::ProcessEnvironment;
use crate::application::ports::UsageError;
use crate::domain::Agent;

pub struct AgentProbe {
    environment: ProcessEnvironment,
    directory: PathBuf,
    bounded: BoundedCommand,
}

impl AgentProbe {
    pub fn new(
        environment: &ProcessEnvironment,
        directory: &Path,
        bounded: BoundedCommand,
    ) -> Self {
        Self {
            environment: environment.clone(),
            directory: directory.to_path_buf(),
            bounded,
        }
    }

    pub fn run(&self, agent: &Agent, arguments: &[&str]) -> Result<String, UsageError> {
        self.create_directory()?;
        let call = Call::new(agent.name(), CallKind::UsageProbe);
        let output = self
            .bounded
            .run(self.command(agent, arguments), &call)
            .map_err(Self::unfinished)?;
        Self::report(agent, &output)
    }

    fn create_directory(&self) -> Result<(), UsageError> {
        fs::create_dir_all(&self.directory).map_err(|error| {
            log::info!(
                "could not create the usage directory {}: {error}",
                self.directory.display()
            );
            UsageError::Unanswered
        })
    }

    fn command(&self, agent: &Agent, arguments: &[&str]) -> Command {
        let variables = AgentVariables::new(
            &self.environment,
            agent.env_names(),
            agent.env_assignments(),
        );
        let mut command = Command::new(agent.command());
        command
            .args(arguments)
            .env_clear()
            .envs(variables.combined())
            .current_dir(&self.directory);
        command
    }

    fn unfinished(error: CommandError) -> UsageError {
        log::info!("{error}");
        match error {
            CommandError::TimedOut { .. } => UsageError::TimedOut,
            CommandError::Spawn { .. } => UsageError::Unanswered,
        }
    }

    fn report(agent: &Agent, output: &CommandOutput) -> Result<String, UsageError> {
        if output.succeeded() {
            return Ok(output.stdout_text());
        }
        log::info!(
            "the agent {} could not report its usage, {}: {}",
            agent.name(),
            output.status_label(),
            output.stderr_text()
        );
        Err(UsageError::Unanswered)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::time::Duration;

    use super::*;
    use crate::adapters::Deadlines;
    use crate::test_support::{ExecutableScript, InheritedPath};

    struct ProbeFixture {
        directory: tempfile::TempDir,
    }

    impl ProbeFixture {
        const RECORDING_AGENT: &'static str = "#!/bin/sh\n\
            pwd > \"$0.cwd\"\n\
            env > \"$0.env\"\n\
            printf '%s\\n' \"$@\" > \"$0.args\"\n\
            echo example-report\n";
        const FAILING_AGENT: &'static str = "#!/bin/sh\necho not logged in >&2\nexit 1\n";
        const SLOW_AGENT: &'static str = "#!/bin/sh\nexec sleep 5\n";
        const SHORT_DEADLINE: Duration = Duration::from_millis(200);

        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn agent(&self, body: &str) -> Agent {
            let path = self.directory.path().join("example-agent");
            ExecutableScript::write(&path, body);
            Agent::new("work", &path.to_string_lossy())
        }

        fn usage_directory(&self) -> PathBuf {
            self.directory.path().join("cache/ringleader/usage")
        }

        fn probe(&self) -> AgentProbe {
            self.probe_within(Deadlines::standard())
        }

        fn probe_within(&self, deadlines: Deadlines) -> AgentProbe {
            let variables = vec![
                (OsString::from("PATH"), InheritedPath::value()),
                (OsString::from("HOME"), OsString::from("/home/example")),
                (OsString::from("EXAMPLE_TOKEN"), OsString::from("inherited")),
                (OsString::from("UNRELATED"), OsString::from("elsewhere")),
            ];
            AgentProbe::new(
                &ProcessEnvironment::new(variables, None),
                &self.usage_directory(),
                BoundedCommand::new(deadlines),
            )
        }

        fn recorded(&self, suffix: &str) -> String {
            let path = self
                .directory
                .path()
                .join(format!("example-agent.{suffix}"));
            fs::read_to_string(path).unwrap()
        }

        fn recorded_variable(&self, name: &str) -> Option<String> {
            let prefix = format!("{name}=");
            self.recorded("env")
                .lines()
                .find_map(|line| line.strip_prefix(&prefix).map(str::to_string))
        }
    }

    #[test]
    fn the_report_is_what_the_agent_prints() {
        let fixture = ProbeFixture::new();
        let agent = fixture.agent(ProbeFixture::RECORDING_AGENT);

        let report = fixture.probe().run(&agent, &["--usage"]).unwrap();

        assert_eq!(report, "example-report\n");
    }

    #[test]
    fn the_probe_passes_only_the_given_arguments() {
        let fixture = ProbeFixture::new();
        let agent = fixture
            .agent(ProbeFixture::RECORDING_AGENT)
            .with_args(&["--model".to_string(), "example".to_string()]);

        fixture.probe().run(&agent, &["-p", "/usage"]).unwrap();

        assert_eq!(fixture.recorded("args"), "-p\n/usage\n");
    }

    #[test]
    fn the_probe_runs_in_the_usage_directory() {
        let fixture = ProbeFixture::new();
        let agent = fixture.agent(ProbeFixture::RECORDING_AGENT);

        fixture.probe().run(&agent, &[]).unwrap();

        assert_eq!(
            fixture.recorded("cwd").trim_end(),
            fixture.usage_directory().to_string_lossy()
        );
    }

    #[test]
    fn the_probe_gets_the_variables_the_agent_sets() {
        let fixture = ProbeFixture::new();
        let agent = fixture
            .agent(ProbeFixture::RECORDING_AGENT)
            .with_env_names(&["EXAMPLE_TOKEN".to_string()])
            .with_env_assignments(&[("EXAMPLE_TOKEN".to_string(), "assigned".to_string())]);

        fixture.probe().run(&agent, &[]).unwrap();

        assert_eq!(
            fixture.recorded_variable("EXAMPLE_TOKEN").as_deref(),
            Some("assigned")
        );
    }

    #[test]
    fn the_probe_gets_the_variables_the_agent_names() {
        let fixture = ProbeFixture::new();
        let agent = fixture
            .agent(ProbeFixture::RECORDING_AGENT)
            .with_env_names(&["EXAMPLE_TOKEN".to_string()]);

        fixture.probe().run(&agent, &[]).unwrap();

        assert_eq!(
            fixture.recorded_variable("EXAMPLE_TOKEN").as_deref(),
            Some("inherited")
        );
    }

    #[test]
    fn the_probe_gets_no_variable_the_agent_does_not_get() {
        let fixture = ProbeFixture::new();
        let agent = fixture.agent(ProbeFixture::RECORDING_AGENT);

        fixture.probe().run(&agent, &[]).unwrap();

        assert_eq!(fixture.recorded_variable("UNRELATED"), None);
    }

    #[test]
    fn a_failing_agent_did_not_answer() {
        let fixture = ProbeFixture::new();
        let agent = fixture.agent(ProbeFixture::FAILING_AGENT);

        assert_eq!(
            fixture.probe().run(&agent, &[]),
            Err(UsageError::Unanswered)
        );
    }

    #[test]
    fn an_agent_that_cannot_start_did_not_answer() {
        let fixture = ProbeFixture::new();
        let agent = Agent::new("work", "/nonexistent/example-agent");

        assert_eq!(
            fixture.probe().run(&agent, &[]),
            Err(UsageError::Unanswered)
        );
    }

    #[test]
    fn an_agent_past_its_deadline_timed_out() {
        let fixture = ProbeFixture::new();
        let agent = fixture.agent(ProbeFixture::SLOW_AGENT);
        let probe = fixture.probe_within(Deadlines::uniform(ProbeFixture::SHORT_DEADLINE));

        assert_eq!(probe.run(&agent, &[]), Err(UsageError::TimedOut));
    }

    #[test]
    fn a_usage_directory_that_cannot_be_made_leaves_the_agent_unasked() {
        let fixture = ProbeFixture::new();
        let agent = fixture.agent(ProbeFixture::RECORDING_AGENT);
        fs::write(fixture.directory.path().join("cache"), "").unwrap();

        assert_eq!(
            fixture.probe().run(&agent, &[]),
            Err(UsageError::Unanswered)
        );
        assert!(!fixture.directory.path().join("example-agent.args").exists());
    }
}

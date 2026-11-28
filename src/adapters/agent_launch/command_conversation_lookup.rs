use std::process::Command;

use super::agent_variables::AgentVariables;
use crate::adapters::BoundedCommand;
use crate::adapters::child_process::bounded_command::{Call, CommandError};
use crate::adapters::child_process::command_output::CommandOutput;
use crate::adapters::child_process::deadlines::CallKind;
use crate::adapters::host::process_environment::ProcessEnvironment;
use crate::application::ports::{ConversationLookup, ConversationLookupError, ConversationQuery};

pub struct CommandConversationLookup {
    environment: ProcessEnvironment,
    bounded: BoundedCommand,
}

impl CommandConversationLookup {
    pub fn new(environment: &ProcessEnvironment, bounded: BoundedCommand) -> Self {
        Self {
            environment: environment.clone(),
            bounded,
        }
    }

    fn command(&self, query: &ConversationQuery) -> Result<Command, ConversationLookupError> {
        let (program, arguments) = query
            .argv()
            .split_first()
            .ok_or(ConversationLookupError::Unanswered)?;
        let agent = query.agent();
        let variables = AgentVariables::new(
            &self.environment,
            agent.env_names(),
            agent.env_assignments(),
        );
        let mut command = Command::new(program);
        command
            .args(arguments)
            .env_clear()
            .envs(variables.combined())
            .current_dir(query.worktree());
        Ok(command)
    }

    fn unfinished(error: CommandError) -> ConversationLookupError {
        log::info!("{error}");
        match error {
            CommandError::TimedOut { .. } => ConversationLookupError::TimedOut,
            CommandError::Spawn { .. } => ConversationLookupError::Unanswered,
        }
    }

    fn conversation(
        query: &ConversationQuery,
        output: &CommandOutput,
    ) -> Result<Option<String>, ConversationLookupError> {
        if !output.succeeded() {
            log::info!(
                "the conversation lookup of agent {} failed, {}: {}",
                query.agent().name(),
                output.status_label(),
                output.stderr_text()
            );
            return Err(ConversationLookupError::Unanswered);
        }
        Ok(Self::first_line(&output.stdout_text()))
    }

    fn first_line(stdout: &str) -> Option<String> {
        stdout
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(str::to_string)
    }
}

impl ConversationLookup for CommandConversationLookup {
    fn find(&self, query: &ConversationQuery) -> Result<Option<String>, ConversationLookupError> {
        let call = Call::new(query.agent().name(), CallKind::ConversationLookup);
        let output = self
            .bounded
            .run(self.command(query)?, &call)
            .map_err(Self::unfinished)?;
        Self::conversation(query, &output)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::adapters::Deadlines;
    use crate::domain::Agent;
    use crate::test_support::{ExecutableScript, InheritedPath};

    struct LookupFixture {
        directory: tempfile::TempDir,
    }

    impl LookupFixture {
        const RECORDING_LOOKUP: &'static str = "#!/bin/sh\n\
            pwd > \"$0.cwd\"\n\
            printf '%s\\n' \"$@\" > \"$0.args\"\n\
            printf '\\n  %s  \\nsecond\\n' \"$EXAMPLE_ACCOUNT\"\n";
        const SILENT_LOOKUP: &'static str = "#!/bin/sh\nexit 0\n";
        const FAILING_LOOKUP: &'static str = "#!/bin/sh\necho no database >&2\nexit 1\n";
        const SLOW_LOOKUP: &'static str = "#!/bin/sh\nexec sleep 5\n";
        const SHORT_DEADLINE: Duration = Duration::from_millis(200);

        fn new() -> Self {
            let fixture = Self {
                directory: tempfile::tempdir().unwrap(),
            };
            fs::create_dir(fixture.worktree()).unwrap();
            fixture
        }

        fn worktree(&self) -> PathBuf {
            self.directory.path().join("example-worktree")
        }

        fn query(&self, body: &str) -> ConversationQuery {
            let script = self.directory.path().join("example-lookup");
            ExecutableScript::write(&script, body);
            let agent = Agent::new("example-agent", "example-agent").with_env_assignments(&[(
                "EXAMPLE_ACCOUNT".to_string(),
                "conversation-7".to_string(),
            )]);
            let argv = vec![
                script.to_string_lossy().into_owned(),
                "fix-auth".to_string(),
            ];
            ConversationQuery::new(&agent, argv, &self.worktree())
        }

        fn lookup(&self) -> CommandConversationLookup {
            self.lookup_within(Deadlines::standard())
        }

        fn lookup_within(&self, deadlines: Deadlines) -> CommandConversationLookup {
            let variables = vec![(OsString::from("PATH"), InheritedPath::value())];
            CommandConversationLookup::new(
                &ProcessEnvironment::new(variables, None),
                BoundedCommand::new(deadlines),
            )
        }

        fn recorded(&self, suffix: &str) -> String {
            let path = self
                .directory
                .path()
                .join(format!("example-lookup.{suffix}"));
            fs::read_to_string(path).unwrap()
        }
    }

    #[test]
    fn the_conversation_is_the_first_non_empty_line_with_the_agent_variables() {
        let fixture = LookupFixture::new();
        let query = fixture.query(LookupFixture::RECORDING_LOOKUP);

        let found = fixture.lookup().find(&query).unwrap();

        assert_eq!(found.as_deref(), Some("conversation-7"));
    }

    #[test]
    fn the_lookup_runs_in_the_worktree_with_its_arguments() {
        let fixture = LookupFixture::new();
        let query = fixture.query(LookupFixture::RECORDING_LOOKUP);

        fixture.lookup().find(&query).unwrap();

        let worktree = fs::canonicalize(fixture.worktree()).unwrap();
        assert_eq!(
            fixture.recorded("cwd").trim_end(),
            worktree.to_string_lossy()
        );
        assert_eq!(fixture.recorded("args"), "fix-auth\n");
    }

    #[test]
    fn a_lookup_that_prints_nothing_finds_no_conversation() {
        let fixture = LookupFixture::new();
        let query = fixture.query(LookupFixture::SILENT_LOOKUP);

        assert_eq!(fixture.lookup().find(&query), Ok(None));
    }

    #[test]
    fn a_failing_lookup_did_not_answer() {
        let fixture = LookupFixture::new();
        let query = fixture.query(LookupFixture::FAILING_LOOKUP);

        assert_eq!(
            fixture.lookup().find(&query),
            Err(ConversationLookupError::Unanswered)
        );
    }

    #[test]
    fn a_lookup_that_cannot_start_did_not_answer() {
        let fixture = LookupFixture::new();
        let agent = Agent::new("example-agent", "example-agent");
        let argv = vec!["/nonexistent/example-lookup".to_string()];
        let query = ConversationQuery::new(&agent, argv, &fixture.worktree());

        assert_eq!(
            fixture.lookup().find(&query),
            Err(ConversationLookupError::Unanswered)
        );
    }

    #[test]
    fn a_lookup_past_its_deadline_timed_out() {
        let fixture = LookupFixture::new();
        let query = fixture.query(LookupFixture::SLOW_LOOKUP);
        let lookup = fixture.lookup_within(Deadlines::uniform(LookupFixture::SHORT_DEADLINE));

        assert_eq!(lookup.find(&query), Err(ConversationLookupError::TimedOut));
    }
}

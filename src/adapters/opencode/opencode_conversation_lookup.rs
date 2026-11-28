use std::path::Path;
use std::sync::Arc;

use serde::Deserialize;

use super::opencode_database::OpencodeDatabase;
use crate::adapters::agent_launch::agent_probe::AgentProbe;
use crate::application::ports::{ConversationLookup, ConversationLookupError, UsageError};
use crate::domain::Agent;

pub struct OpencodeConversationLookup {
    probe: Arc<AgentProbe>,
    database: Arc<dyn OpencodeDatabase>,
}

impl OpencodeConversationLookup {
    const PROGRAM: &'static str = "opencode";
    const DATABASE_ARGUMENTS: [&'static str; 4] = ["--pure", "db", "--format", "json"];

    pub fn new(probe: Arc<AgentProbe>, database: Arc<dyn OpencodeDatabase>) -> Self {
        Self { probe, database }
    }

    fn rows(&self, agent: &Agent, query: &str) -> Result<String, ConversationLookupError> {
        let direct = self.database.read(agent, query).unwrap_or_else(|error| {
            log::info!("{error}, asking opencode instead");
            None
        });
        match direct {
            Some(output) => Ok(output),
            None => self.ask_opencode(agent, query),
        }
    }

    fn ask_opencode(&self, agent: &Agent, query: &str) -> Result<String, ConversationLookupError> {
        let mut arguments = Self::DATABASE_ARGUMENTS.to_vec();
        arguments.push(query);
        self.probe.run(agent, &arguments).map_err(Self::unanswered)
    }

    fn unanswered(error: UsageError) -> ConversationLookupError {
        match error {
            UsageError::TimedOut => ConversationLookupError::TimedOut,
            UsageError::Unanswered | UsageError::Unreadable => ConversationLookupError::Unanswered,
        }
    }
}

impl ConversationLookup for OpencodeConversationLookup {
    fn find(
        &self,
        agent: &Agent,
        worktree: &Path,
    ) -> Result<Option<String>, ConversationLookupError> {
        if agent.command_name() != Self::PROGRAM {
            return Ok(None);
        }
        let output = self.rows(agent, &OpencodeConversation::query(worktree))?;
        OpencodeConversation::parse(&output)
    }
}

#[derive(Debug, Deserialize)]
struct OpencodeConversation {
    id: String,
}

impl OpencodeConversation {
    fn query(worktree: &Path) -> String {
        let directory = worktree.to_string_lossy().replace('\'', "''");
        format!(
            "select id from session where parent_id is null and directory = '{directory}' \
             order by time_updated desc limit 1"
        )
    }

    fn parse(output: &str) -> Result<Option<String>, ConversationLookupError> {
        if output.trim().is_empty() {
            return Ok(None);
        }
        let rows: Vec<Self> = serde_json::from_str(output).map_err(|error| {
            log::info!("could not read opencode's conversation: {error}");
            ConversationLookupError::Unanswered
        })?;
        Ok(rows.into_iter().next().map(|row| row.id))
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::adapters::host::process_environment::ProcessEnvironment;
    use crate::adapters::opencode::fake_opencode_database::FakeOpencodeDatabase;
    use crate::adapters::opencode::opencode_database::OpencodeDatabaseError;
    use crate::adapters::sqlite::null_opencode_database::NullOpencodeDatabase;
    use crate::adapters::{BoundedCommand, Deadlines};
    use crate::test_support::{ExecutableScript, InheritedPath};

    struct LookupFixture {
        directory: tempfile::TempDir,
    }

    impl LookupFixture {
        const FOUND: &'static str = r#"[{"id":"ses_example"}]"#;
        const WORKTREE: &'static str = "/worktrees/example-project/fix-auth";
        const SHORT_DEADLINE: Duration = Duration::from_millis(200);

        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn opencode_printing(&self, output: &str) -> Agent {
            let printed = self.directory.path().join("printed.json");
            fs::write(&printed, output).unwrap();
            let body = format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\ncat '{}'\n",
                printed.display()
            );
            self.agent_running("opencode", &body)
        }

        fn agent_running(&self, program: &str, body: &str) -> Agent {
            let path = self.directory.path().join(program);
            ExecutableScript::write(&path, body);
            Agent::new("work", &path.to_string_lossy())
        }

        fn lookup(&self, database: Arc<dyn OpencodeDatabase>) -> OpencodeConversationLookup {
            self.lookup_within(database, Deadlines::standard())
        }

        fn lookup_within(
            &self,
            database: Arc<dyn OpencodeDatabase>,
            deadlines: Deadlines,
        ) -> OpencodeConversationLookup {
            let variables = vec![(OsString::from("PATH"), InheritedPath::value())];
            let probe = Arc::new(AgentProbe::new(
                &ProcessEnvironment::new(variables, None),
                &self.directory.path().join("usage"),
                BoundedCommand::new(deadlines),
            ));
            OpencodeConversationLookup::new(probe, database)
        }

        fn without_database(&self) -> OpencodeConversationLookup {
            self.lookup(Arc::new(NullOpencodeDatabase))
        }

        fn recorded_arguments(&self, program: &str) -> Option<Vec<String>> {
            let path = self.directory.path().join(format!("{program}.args"));
            let text = fs::read_to_string(path).ok()?;
            Some(text.lines().map(str::to_string).collect())
        }

        fn worktree() -> PathBuf {
            PathBuf::from(Self::WORKTREE)
        }
    }

    #[test]
    fn the_conversation_is_the_session_the_database_finds() {
        let fixture = LookupFixture::new();
        let database = FakeOpencodeDatabase::answering(Ok(Some(LookupFixture::FOUND.to_string())));

        let found = fixture
            .lookup(Arc::new(database))
            .find(&fixture.opencode_printing("[]"), &LookupFixture::worktree());

        assert_eq!(found, Ok(Some("ses_example".to_string())));
    }

    #[test]
    fn the_query_asks_for_the_newest_root_session_of_the_worktree() {
        let fixture = LookupFixture::new();
        let database = Arc::new(FakeOpencodeDatabase::answering(Ok(Some(String::new()))));

        fixture
            .lookup(database.clone())
            .find(&fixture.opencode_printing("[]"), &LookupFixture::worktree())
            .unwrap();

        assert_eq!(
            database.queries(),
            [format!(
                "select id from session where parent_id is null and directory = '{}' \
                 order by time_updated desc limit 1",
                LookupFixture::WORKTREE
            )]
        );
    }

    #[test]
    fn a_quote_in_the_worktree_path_stays_inside_the_query_string() {
        let fixture = LookupFixture::new();
        let database = Arc::new(FakeOpencodeDatabase::answering(Ok(Some(String::new()))));

        fixture
            .lookup(database.clone())
            .find(
                &fixture.opencode_printing("[]"),
                Path::new("/worktrees/it's"),
            )
            .unwrap();

        assert!(database.queries()[0].contains("directory = '/worktrees/it''s'"));
    }

    #[test]
    fn a_worktree_without_a_session_has_no_conversation() {
        let fixture = LookupFixture::new();
        let database = FakeOpencodeDatabase::answering(Ok(Some(String::new())));

        let found = fixture
            .lookup(Arc::new(database))
            .find(&fixture.opencode_printing("[]"), &LookupFixture::worktree());

        assert_eq!(found, Ok(None));
    }

    #[test]
    fn an_agent_that_is_not_opencode_has_no_conversation_and_runs_nothing() {
        let fixture = LookupFixture::new();
        let database = Arc::new(FakeOpencodeDatabase::answering(Ok(Some(
            LookupFixture::FOUND.to_string(),
        ))));
        let agent = fixture.agent_running("claude", "#!/bin/sh\ntouch \"$0.args\"\n");

        let found = fixture
            .lookup(database.clone())
            .find(&agent, &LookupFixture::worktree());

        assert_eq!(found, Ok(None));
        assert!(database.queries().is_empty());
        assert_eq!(fixture.recorded_arguments("claude"), None);
    }

    #[test]
    fn without_the_database_opencode_answers_the_query_itself() {
        let fixture = LookupFixture::new();
        let agent = fixture.opencode_printing(LookupFixture::FOUND);

        let found = fixture
            .without_database()
            .find(&agent, &LookupFixture::worktree());

        assert_eq!(found, Ok(Some("ses_example".to_string())));
        let arguments = fixture.recorded_arguments("opencode").unwrap();
        assert_eq!(arguments[..4], ["--pure", "db", "--format", "json"]);
        assert!(arguments[4].starts_with("select id from session"));
    }

    #[test]
    fn a_failed_database_read_asks_opencode_instead() {
        let fixture = LookupFixture::new();
        let database =
            FakeOpencodeDatabase::answering(Err(OpencodeDatabaseError::new("database is locked")));
        let agent = fixture.opencode_printing(LookupFixture::FOUND);

        let found = fixture
            .lookup(Arc::new(database))
            .find(&agent, &LookupFixture::worktree());

        assert_eq!(found, Ok(Some("ses_example".to_string())));
    }

    #[test]
    fn an_opencode_past_its_deadline_timed_out() {
        let fixture = LookupFixture::new();
        let agent = fixture.agent_running("opencode", "#!/bin/sh\nexec sleep 5\n");
        let lookup = fixture.lookup_within(
            Arc::new(NullOpencodeDatabase),
            Deadlines::uniform(LookupFixture::SHORT_DEADLINE),
        );

        let found = lookup.find(&agent, &LookupFixture::worktree());

        assert_eq!(found, Err(ConversationLookupError::TimedOut));
    }

    #[test]
    fn an_unreadable_answer_did_not_answer() {
        let fixture = LookupFixture::new();
        let agent = fixture.opencode_printing("not json");

        let found = fixture
            .without_database()
            .find(&agent, &LookupFixture::worktree());

        assert_eq!(found, Err(ConversationLookupError::Unanswered));
    }
}

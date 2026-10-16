use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use super::null_opencode_database::NullOpencodeDatabase;
use crate::adapters::BoundedCommand;
use crate::adapters::agent_launch::agent_variables::AgentVariables;
use crate::adapters::child_process::bounded_command::Call;
use crate::adapters::child_process::deadlines::CallKind;
use crate::adapters::host::process_environment::ProcessEnvironment;
use crate::adapters::opencode::opencode_database::{OpencodeDatabase, OpencodeDatabaseError};
use crate::application::ports::HostProbe;
use crate::domain::Agent;

pub struct SqliteOpencodeDatabase {
    program: OsString,
    environment: ProcessEnvironment,
    bounded: BoundedCommand,
}

impl SqliteOpencodeDatabase {
    const PROGRAM: &'static str = "sqlite3";
    const SUBCOMMAND: &'static str = "opencode database";
    const READ_ONLY_JSON: [&'static str; 4] = ["-init", "/dev/null", "-readonly", "-json"];
    const DATA_HOME: &'static str = "XDG_DATA_HOME";
    const HOME: &'static str = "HOME";
    const DEFAULT_DATA_HOME: &'static str = ".local/share";
    const DATABASE: &'static str = "opencode/opencode.db";

    pub fn when_installed(
        host: &dyn HostProbe,
        environment: &ProcessEnvironment,
        bounded: BoundedCommand,
    ) -> Arc<dyn OpencodeDatabase> {
        match host.resolve_command(Self::PROGRAM) {
            Some(program) => Arc::new(Self {
                program: program.into_os_string(),
                environment: environment.clone(),
                bounded,
            }),
            None => Arc::new(NullOpencodeDatabase),
        }
    }

    fn query(&self, database: &Path, query: &str) -> Result<String, OpencodeDatabaseError> {
        let call = Call::new(Self::SUBCOMMAND, CallKind::SqliteQuery);
        let command = self.command(database, query);
        let output = self
            .bounded
            .run(command, &call)
            .map_err(|error| OpencodeDatabaseError::new(&error.to_string()))?;
        if !output.succeeded() {
            let detail = format!("{}: {}", database.display(), output.stderr_text());
            return Err(OpencodeDatabaseError::new(&detail));
        }
        Ok(output.stdout_text())
    }

    fn command(&self, database: &Path, query: &str) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(Self::READ_ONLY_JSON)
            .arg(database)
            .arg(query)
            .env_clear()
            .envs(self.environment.baseline().variables().iter().cloned());
        command
    }

    fn database(variables: &AgentVariables) -> Option<PathBuf> {
        let data_home = variables
            .value(Self::DATA_HOME)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                Some(PathBuf::from(variables.value(Self::HOME)?).join(Self::DEFAULT_DATA_HOME))
            })?;
        Some(data_home.join(Self::DATABASE))
    }
}

impl OpencodeDatabase for SqliteOpencodeDatabase {
    fn read(&self, agent: &Agent, query: &str) -> Result<Option<String>, OpencodeDatabaseError> {
        let variables = AgentVariables::new(
            &self.environment,
            agent.env_names(),
            agent.env_assignments(),
        );
        let Some(database) = Self::database(&variables) else {
            return Ok(None);
        };
        self.query(&database, query).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use super::*;
    use crate::adapters::Deadlines;
    use crate::test_support::{ExecutableScript, FakeHostProbe, InheritedPath};

    struct DatabaseFixture {
        directory: tempfile::TempDir,
    }

    impl DatabaseFixture {
        const RECORDING_SQLITE: &'static str = "#!/bin/sh\n\
            printf '%s\\n' \"$@\" > \"$0.args\"\n\
            env > \"$0.env\"\n\
            echo '[{\"one\":1}]'\n";
        const FAILING_SQLITE: &'static str =
            "#!/bin/sh\necho 'Error: unable to open database' >&2\nexit 1\n";
        const SLOW_SQLITE: &'static str = "#!/bin/sh\nexec sleep 5\n";
        const SHORT_DEADLINE: Duration = Duration::from_millis(200);
        const QUERY: &'static str = "select 1 as one";

        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn sqlite(&self, body: &str) -> PathBuf {
            let path = self.directory.path().join("sqlite3");
            ExecutableScript::write(&path, body);
            path
        }

        fn database(&self, body: &str, variables: &[(&str, &str)]) -> Arc<dyn OpencodeDatabase> {
            self.database_within(body, variables, Deadlines::standard())
        }

        fn database_within(
            &self,
            body: &str,
            variables: &[(&str, &str)],
            deadlines: Deadlines,
        ) -> Arc<dyn OpencodeDatabase> {
            let host = FakeHostProbe::default().with_command_at("sqlite3", &self.sqlite(body));
            SqliteOpencodeDatabase::when_installed(
                &host,
                &Self::environment(variables),
                BoundedCommand::new(deadlines),
            )
        }

        fn environment(variables: &[(&str, &str)]) -> ProcessEnvironment {
            let mut pairs = vec![(OsString::from("PATH"), InheritedPath::value())];
            pairs.extend(
                variables
                    .iter()
                    .map(|(name, value)| (OsString::from(name), OsString::from(value))),
            );
            ProcessEnvironment::new(pairs, None)
        }

        fn agent() -> Agent {
            Agent::new("opencode", "opencode")
        }

        fn arguments(&self) -> Vec<String> {
            fs::read_to_string(self.directory.path().join("sqlite3.args"))
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect()
        }
    }

    #[test]
    fn the_read_is_what_sqlite_prints() {
        let fixture = DatabaseFixture::new();
        let database = fixture.database(
            DatabaseFixture::RECORDING_SQLITE,
            &[("HOME", "/home/example")],
        );

        let output = database.read(&DatabaseFixture::agent(), DatabaseFixture::QUERY);

        assert_eq!(output, Ok(Some("[{\"one\":1}]\n".to_string())));
    }

    #[test]
    fn the_database_opens_read_only_without_the_users_sqliterc() {
        let fixture = DatabaseFixture::new();
        let database = fixture.database(
            DatabaseFixture::RECORDING_SQLITE,
            &[("HOME", "/home/example")],
        );

        database
            .read(&DatabaseFixture::agent(), DatabaseFixture::QUERY)
            .unwrap();

        assert_eq!(
            fixture.arguments()[..4],
            ["-init", "/dev/null", "-readonly", "-json"]
        );
        assert_eq!(fixture.arguments()[5], DatabaseFixture::QUERY);
    }

    #[test]
    fn the_database_is_in_the_data_home() {
        let fixture = DatabaseFixture::new();
        let variables = [
            ("HOME", "/home/example"),
            ("XDG_DATA_HOME", "/data/example"),
        ];
        let database = fixture.database(DatabaseFixture::RECORDING_SQLITE, &variables);

        database
            .read(&DatabaseFixture::agent(), DatabaseFixture::QUERY)
            .unwrap();

        assert_eq!(fixture.arguments()[4], "/data/example/opencode/opencode.db");
    }

    #[test]
    fn without_a_data_home_the_database_is_under_home() {
        let fixture = DatabaseFixture::new();
        let database = fixture.database(
            DatabaseFixture::RECORDING_SQLITE,
            &[("HOME", "/home/example")],
        );

        database
            .read(&DatabaseFixture::agent(), DatabaseFixture::QUERY)
            .unwrap();

        assert_eq!(
            fixture.arguments()[4],
            "/home/example/.local/share/opencode/opencode.db"
        );
    }

    #[test]
    fn a_relative_data_home_is_ignored() {
        let fixture = DatabaseFixture::new();
        let variables = [
            ("HOME", "/home/example"),
            ("XDG_DATA_HOME", "relative-data"),
        ];
        let database = fixture.database(DatabaseFixture::RECORDING_SQLITE, &variables);

        database
            .read(&DatabaseFixture::agent(), DatabaseFixture::QUERY)
            .unwrap();

        assert_eq!(
            fixture.arguments()[4],
            "/home/example/.local/share/opencode/opencode.db"
        );
    }

    #[test]
    fn the_data_home_the_agent_sets_picks_the_database() {
        let fixture = DatabaseFixture::new();
        let database = fixture.database(
            DatabaseFixture::RECORDING_SQLITE,
            &[("HOME", "/home/example")],
        );
        let agent = DatabaseFixture::agent()
            .with_env_assignments(&[("XDG_DATA_HOME".to_string(), "/accounts/work".to_string())]);

        database.read(&agent, DatabaseFixture::QUERY).unwrap();

        assert_eq!(
            fixture.arguments()[4],
            "/accounts/work/opencode/opencode.db"
        );
    }

    #[test]
    fn sqlite_gets_none_of_the_agents_named_variables() {
        let fixture = DatabaseFixture::new();
        let variables = [
            ("HOME", "/home/example"),
            ("EXAMPLE_TOKEN", "example-secret"),
        ];
        let database = fixture.database(DatabaseFixture::RECORDING_SQLITE, &variables);
        let agent = DatabaseFixture::agent().with_env_names(&["EXAMPLE_TOKEN".to_string()]);

        database.read(&agent, DatabaseFixture::QUERY).unwrap();

        let environment = fs::read_to_string(fixture.directory.path().join("sqlite3.env")).unwrap();
        assert!(!environment.contains("example-secret"));
    }

    #[test]
    fn without_a_home_nothing_is_read() {
        let fixture = DatabaseFixture::new();
        let database = fixture.database(DatabaseFixture::RECORDING_SQLITE, &[]);

        assert_eq!(
            database.read(&DatabaseFixture::agent(), DatabaseFixture::QUERY),
            Ok(None)
        );
        assert!(fixture.arguments().is_empty());
    }

    #[test]
    fn a_failing_sqlite_is_a_failed_read() {
        let fixture = DatabaseFixture::new();
        let database = fixture.database(
            DatabaseFixture::FAILING_SQLITE,
            &[("HOME", "/home/example")],
        );

        assert!(
            database
                .read(&DatabaseFixture::agent(), DatabaseFixture::QUERY)
                .is_err()
        );
    }

    #[test]
    fn a_sqlite_past_its_deadline_is_a_failed_read() {
        let fixture = DatabaseFixture::new();
        let database = fixture.database_within(
            DatabaseFixture::SLOW_SQLITE,
            &[("HOME", "/home/example")],
            Deadlines::uniform(DatabaseFixture::SHORT_DEADLINE),
        );

        assert!(
            database
                .read(&DatabaseFixture::agent(), DatabaseFixture::QUERY)
                .is_err()
        );
    }

    #[test]
    fn without_sqlite_nothing_is_read() {
        let database = SqliteOpencodeDatabase::when_installed(
            &FakeHostProbe::default(),
            &DatabaseFixture::environment(&[("HOME", "/home/example")]),
            BoundedCommand::new(Deadlines::standard()),
        );

        assert_eq!(
            database.read(&DatabaseFixture::agent(), DatabaseFixture::QUERY),
            Ok(None)
        );
    }
}

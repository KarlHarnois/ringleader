use std::ffi::OsString;
use std::process::Command;
use std::sync::Arc;

use super::null_provider_quota::NullProviderQuota;
use super::ollama_balance::OllamaBalance;
use crate::adapters::BoundedCommand;
use crate::adapters::agent_launch::agent_variables::AgentVariables;
use crate::adapters::child_process::bounded_command::{Call, CommandError};
use crate::adapters::child_process::command_output::CommandOutput;
use crate::adapters::child_process::deadlines::CallKind;
use crate::adapters::host::process_environment::ProcessEnvironment;
use crate::adapters::opencode::provider_quota::{ProviderQuota, QuotaError};
use crate::application::ports::HostProbe;
use crate::domain::{Agent, UsageRow};

pub struct OllamaCloudQuota {
    program: OsString,
    environment: ProcessEnvironment,
    bounded: BoundedCommand,
}

impl OllamaCloudQuota {
    const PROVIDER: &'static str = "ollama-cloud";
    const KEY_VARIABLE: &'static str = "OLLAMA_API_KEY";
    const PROGRAM: &'static str = "curl";
    const SUBCOMMAND: &'static str = "balance";
    const BALANCE_URL: &'static str = "https://ollama.com/api/balance";
    const ARGUMENTS: [&'static str; 12] = [
        "-q",
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--max-time",
        "10",
        "--header",
        "@-",
        "--write-out",
        "\n%{http_code}",
        Self::BALANCE_URL,
    ];
    const STATUS_SEPARATOR: char = '\n';
    const ANSWERED: &'static str = "200";
    const REFUSED: [&'static str; 2] = ["401", "403"];
    const THROTTLED: &'static str = "429";

    pub fn when_installed(
        host: &dyn HostProbe,
        environment: &ProcessEnvironment,
        bounded: BoundedCommand,
    ) -> Arc<dyn ProviderQuota> {
        match host.resolve_command(Self::PROGRAM) {
            Some(program) => Arc::new(Self {
                program: program.into_os_string(),
                environment: environment.clone(),
                bounded,
            }),
            None => Arc::new(NullProviderQuota),
        }
    }

    fn key_of(&self, agent: &Agent) -> Option<String> {
        let variables = AgentVariables::new(
            &self.environment,
            agent.env_names(),
            agent.env_assignments(),
        );
        let key = variables.value(Self::KEY_VARIABLE)?.to_str()?;
        (!key.is_empty()).then(|| key.to_string())
    }

    fn query(&self, key: &str) -> Result<Vec<UsageRow>, QuotaError> {
        let call = Call::new(Self::SUBCOMMAND, CallKind::CurlQuery)
            .with_input(Self::authorization_header(key).as_bytes());
        let output = self
            .bounded
            .run(self.command(), &call)
            .map_err(Self::unfinished)?;
        Self::answer(&output)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(Self::ARGUMENTS)
            .env_clear()
            .envs(self.environment.baseline().variables().iter().cloned());
        command
    }

    fn authorization_header(key: &str) -> String {
        format!("Authorization: Bearer {key}\n")
    }

    fn unfinished(error: CommandError) -> QuotaError {
        log::info!("{error}");
        match error {
            CommandError::TimedOut { .. } => QuotaError::TimedOut,
            CommandError::Spawn { .. } => QuotaError::Unanswered,
        }
    }

    fn answer(output: &CommandOutput) -> Result<Vec<UsageRow>, QuotaError> {
        let text = output.stdout_text();
        let (body, status) = text.rsplit_once(Self::STATUS_SEPARATOR).unwrap_or_default();
        match status {
            Self::ANSWERED => OllamaBalance::parse(body),
            Self::THROTTLED => Err(QuotaError::Throttled),
            refused if Self::REFUSED.contains(&refused) => Err(QuotaError::Refused),
            _ => {
                log::info!(
                    "Ollama answered the balance query with status {status}: {}",
                    output.stderr_text()
                );
                Err(QuotaError::Unanswered)
            }
        }
    }
}

impl ProviderQuota for OllamaCloudQuota {
    fn measure(&self, agent: &Agent, provider: &str) -> Result<Option<Vec<UsageRow>>, QuotaError> {
        if provider != Self::PROVIDER {
            return Ok(None);
        }
        let Some(key) = self.key_of(agent) else {
            return Ok(None);
        };
        self.query(&key).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::*;
    use crate::adapters::Deadlines;
    use crate::test_support::{ExecutableScript, FakeHostProbe, InheritedPath};

    struct QuotaFixture {
        directory: tempfile::TempDir,
    }

    impl QuotaFixture {
        const KEY: &'static str = "example-key";
        const SLOW_CURL: &'static str = "#!/bin/sh\nexec sleep 5\n";
        const SHORT_DEADLINE: Duration = Duration::from_millis(200);

        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn agent() -> Agent {
            Agent::new("opencode", "opencode").with_env_names(&["OLLAMA_API_KEY".to_string()])
        }

        fn credits() -> PathBuf {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ollama-balance-credits.json")
        }

        fn curl_answering(&self, status: &str, body: &Path) -> PathBuf {
            self.curl(&format!(
                "#!/bin/sh\n\
                 printf '%s\\n' \"$@\" > \"$0.args\"\n\
                 cat > \"$0.stdin\"\n\
                 env > \"$0.env\"\n\
                 cat '{}'\n\
                 printf '\\n{status}'\n",
                body.display()
            ))
        }

        fn curl(&self, body: &str) -> PathBuf {
            let path = self.directory.path().join("curl");
            ExecutableScript::write(&path, body);
            path
        }

        fn body(&self, text: &str) -> PathBuf {
            let path = self.directory.path().join("body.json");
            fs::write(&path, text).unwrap();
            path
        }

        fn environment(key: Option<&str>) -> ProcessEnvironment {
            let mut variables = vec![
                (OsString::from("PATH"), InheritedPath::value()),
                (OsString::from("UNRELATED"), OsString::from("elsewhere")),
            ];
            variables
                .extend(key.map(|key| (OsString::from("OLLAMA_API_KEY"), OsString::from(key))));
            ProcessEnvironment::new(variables, None)
        }

        fn quota(
            host: &FakeHostProbe,
            key: Option<&str>,
            deadlines: Deadlines,
        ) -> Arc<dyn ProviderQuota> {
            OllamaCloudQuota::when_installed(
                host,
                &Self::environment(key),
                BoundedCommand::new(deadlines),
            )
        }

        fn with_curl(curl: &Path) -> Arc<dyn ProviderQuota> {
            let host = FakeHostProbe::default().with_command_at("curl", curl);
            Self::quota(&host, Some(Self::KEY), Deadlines::standard())
        }

        fn answering(&self, status: &str) -> Arc<dyn ProviderQuota> {
            Self::with_curl(&self.curl_answering(status, &Self::credits()))
        }

        fn measured(&self, status: &str) -> Result<Option<Vec<UsageRow>>, QuotaError> {
            self.answering(status)
                .measure(&QuotaFixture::agent(), "ollama-cloud")
        }

        fn recorded(&self, suffix: &str) -> Option<String> {
            fs::read_to_string(self.directory.path().join(format!("curl.{suffix}"))).ok()
        }
    }

    #[test]
    fn another_provider_has_no_quota() {
        let fixture = QuotaFixture::new();

        assert_eq!(
            fixture
                .answering("200")
                .measure(&QuotaFixture::agent(), "openrouter"),
            Ok(None)
        );
        assert_eq!(fixture.recorded("args"), None);
    }

    #[test]
    fn an_agent_that_does_not_get_the_key_has_no_quota() {
        let fixture = QuotaFixture::new();
        let agent = Agent::new("opencode", "opencode");

        assert_eq!(
            fixture.answering("200").measure(&agent, "ollama-cloud"),
            Ok(None)
        );
        assert_eq!(fixture.recorded("args"), None);
    }

    #[test]
    fn the_key_the_agent_sets_is_the_one_sent() {
        let fixture = QuotaFixture::new();
        let agent = Agent::new("opencode", "opencode").with_env_assignments(&[(
            "OLLAMA_API_KEY".to_string(),
            "work-example-key".to_string(),
        )]);

        fixture
            .answering("200")
            .measure(&agent, "ollama-cloud")
            .unwrap();

        assert_eq!(
            fixture.recorded("stdin").unwrap(),
            "Authorization: Bearer work-example-key\n"
        );
    }

    #[test]
    fn a_balance_becomes_quota_rows() {
        let rows = QuotaFixture::new().measured("200").unwrap().unwrap();

        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn the_key_reaches_curl_on_its_standard_input() {
        let fixture = QuotaFixture::new();

        fixture.measured("200").unwrap();

        assert_eq!(
            fixture.recorded("stdin").unwrap(),
            "Authorization: Bearer example-key\n"
        );
    }

    #[test]
    fn the_key_stays_out_of_curls_arguments_and_environment() {
        let fixture = QuotaFixture::new();

        fixture.measured("200").unwrap();

        assert!(
            !fixture
                .recorded("args")
                .unwrap()
                .contains(QuotaFixture::KEY)
        );
        assert!(!fixture.recorded("env").unwrap().contains(QuotaFixture::KEY));
    }

    #[test]
    fn curl_gets_only_the_baseline_variables() {
        let fixture = QuotaFixture::new();

        fixture.measured("200").unwrap();

        assert!(!fixture.recorded("env").unwrap().contains("UNRELATED"));
    }

    #[test]
    fn curl_skips_the_users_curlrc_and_asks_over_https_only() {
        let fixture = QuotaFixture::new();

        fixture.measured("200").unwrap();

        let arguments = fixture.recorded("args").unwrap();
        assert!(arguments.starts_with("-q\n"));
        assert!(arguments.contains("--proto\n=https\n"));
        assert!(arguments.ends_with("https://ollama.com/api/balance\n"));
    }

    #[test]
    fn a_rejected_or_suspended_key_is_refused() {
        for status in ["401", "403"] {
            assert_eq!(
                QuotaFixture::new().measured(status),
                Err(QuotaError::Refused),
                "{status}"
            );
        }
    }

    #[test]
    fn a_rate_limited_query_is_throttled() {
        assert_eq!(
            QuotaFixture::new().measured("429"),
            Err(QuotaError::Throttled)
        );
    }

    #[test]
    fn an_unavailable_service_did_not_answer() {
        assert_eq!(
            QuotaFixture::new().measured("503"),
            Err(QuotaError::Unanswered)
        );
    }

    #[test]
    fn output_without_a_status_did_not_answer() {
        let fixture = QuotaFixture::new();
        let curl = fixture.curl("#!/bin/sh\ncat > /dev/null\nprintf 'no status'\n");

        assert_eq!(
            QuotaFixture::with_curl(&curl).measure(&QuotaFixture::agent(), "ollama-cloud"),
            Err(QuotaError::Unanswered)
        );
    }

    #[test]
    fn an_unreadable_balance_is_unreadable() {
        let fixture = QuotaFixture::new();
        let curl = fixture.curl_answering("200", &fixture.body("<html>"));

        assert_eq!(
            QuotaFixture::with_curl(&curl).measure(&QuotaFixture::agent(), "ollama-cloud"),
            Err(QuotaError::Unreadable)
        );
    }

    #[test]
    fn a_curl_that_cannot_start_did_not_answer() {
        let curl = Path::new("/nonexistent/curl");

        assert_eq!(
            QuotaFixture::with_curl(curl).measure(&QuotaFixture::agent(), "ollama-cloud"),
            Err(QuotaError::Unanswered)
        );
    }

    #[test]
    fn a_curl_past_its_deadline_timed_out() {
        let fixture = QuotaFixture::new();
        let host = FakeHostProbe::default()
            .with_command_at("curl", &fixture.curl(QuotaFixture::SLOW_CURL));
        let deadlines = Deadlines::uniform(QuotaFixture::SHORT_DEADLINE);

        let quota = QuotaFixture::quota(&host, Some(QuotaFixture::KEY), deadlines);

        assert_eq!(
            quota.measure(&QuotaFixture::agent(), "ollama-cloud"),
            Err(QuotaError::TimedOut)
        );
    }

    #[test]
    fn without_curl_the_quota_is_off() {
        let quota = QuotaFixture::quota(
            &FakeHostProbe::default(),
            Some(QuotaFixture::KEY),
            Deadlines::standard(),
        );

        assert_eq!(
            quota.measure(&QuotaFixture::agent(), "ollama-cloud"),
            Ok(None)
        );
    }

    #[test]
    fn without_a_key_the_quota_is_off() {
        for key in [None, Some("")] {
            let fixture = QuotaFixture::new();
            let curl = fixture.curl_answering("200", &QuotaFixture::credits());
            let host = FakeHostProbe::default().with_command_at("curl", &curl);

            let quota = QuotaFixture::quota(&host, key, Deadlines::standard());

            assert_eq!(
                quota.measure(&QuotaFixture::agent(), "ollama-cloud"),
                Ok(None),
                "{key:?}"
            );
        }
    }
}

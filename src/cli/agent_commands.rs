use super::agent_list_report::AgentListReport;
use super::{CliSession, CommandStop};
use crate::application::SessionService;

pub struct AgentCommands<'a> {
    service: &'a SessionService,
    session: &'a mut CliSession,
}

impl<'a> AgentCommands<'a> {
    pub fn new(service: &'a SessionService, session: &'a mut CliSession) -> Self {
        Self { service, session }
    }

    pub fn list(&mut self) -> Result<u8, CommandStop> {
        let report = AgentListReport::new(self.service.config().agents());
        self.session.output().reply(&report)?;
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, CliExit};
    use crate::domain::{Agent, BuildVersion};
    use crate::test_support::{CliHarness, ServiceFixture};

    struct AgentFixture;

    impl AgentFixture {
        fn with_and_without_plan_mode() -> ServiceFixture {
            let config = ServiceFixture::config().with_agents(vec![
                ServiceFixture::agent(),
                Agent::new("other", "other-agent"),
            ]);
            ServiceFixture::new().with_config(config)
        }

        fn without_agents() -> ServiceFixture {
            ServiceFixture::new().with_config(ServiceFixture::config().with_agents(vec![]))
        }
    }

    #[test]
    fn agent_list_json_matches_the_pinned_envelope() {
        let harness = CliHarness::new(AgentFixture::with_and_without_plan_mode());

        let exit = harness.run(&["agent", "list", "--json"]);

        let expected = include_str!("../../tests/fixtures/contract/agent-list.json")
            .replace("<build>", BuildVersion::CURRENT.text());
        assert_eq!(harness.stdout(), expected);
        assert_eq!(harness.stderr(), "");
        assert_eq!(exit, CliExit::Code(0));
    }

    #[test]
    fn agent_list_json_without_agents_prints_an_empty_result() {
        let harness = CliHarness::new(AgentFixture::without_agents());

        harness.run(&["agent", "list", "--json"]);

        let json: serde_json::Value = serde_json::from_str(harness.stdout().trim()).unwrap();
        assert!(json["result"]["agents"].as_array().unwrap().is_empty());
    }

    #[test]
    fn agent_list_marks_plan_support_in_text_form() {
        let harness = CliHarness::new(AgentFixture::with_and_without_plan_mode());

        harness.run(&["agent", "list"]);

        let stdout = harness.stdout();
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("claude "));
        assert!(lines[0].ends_with("claude  plan"));
        assert!(lines[1].starts_with("other "));
        assert!(lines[1].ends_with("other-agent"));
    }

    #[test]
    fn agent_list_rejects_a_lock_timeout() {
        assert!(
            Cli::try_parse_from(["ringleader", "agent", "list", "--lock-timeout", "5"]).is_err()
        );
    }
}

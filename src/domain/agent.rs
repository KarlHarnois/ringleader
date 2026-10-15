use super::agent_mode::AgentMode;
use crate::domain::SessionName;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    name: String,
    command: String,
    args: Vec<String>,
    env_names: Vec<String>,
    env_assignments: Vec<(String, String)>,
    prompt_args: Vec<String>,
    plan_args: Vec<String>,
    resume_args: Vec<String>,
    startup_input: Vec<String>,
    rename_input: Vec<String>,
}

impl Agent {
    pub const PROMPT_PLACEHOLDER: &'static str = "{prompt}";
    pub const NAME_PLACEHOLDER: &'static str = "{name}";
    const BASELINE_VARIABLES: [&'static str; 33] = [
        "PATH",
        "HOME",
        "USER",
        "LOGNAME",
        "SHELL",
        "LANG",
        "TZ",
        "TMPDIR",
        "SSH_AUTH_SOCK",
        "KRB5CCNAME",
        "TERMINFO",
        "TERMINFO_DIRS",
        "LOCALE_ARCHIVE",
        "DISPLAY",
        "XAUTHORITY",
        "WAYLAND_DISPLAY",
        "DBUS_SESSION_BUS_ADDRESS",
        "BROWSER",
        "EDITOR",
        "VISUAL",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "NIX_SSL_CERT_FILE",
        "NODE_EXTRA_CA_CERTS",
        "GNUPGHOME",
    ];
    const BASELINE_PREFIXES: [&'static str; 2] = ["LC_", "XDG_"];

    pub fn is_baseline_variable(name: &str) -> bool {
        Self::BASELINE_VARIABLES.contains(&name)
            || Self::BASELINE_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix))
    }

    pub fn new(name: &str, command: &str) -> Self {
        Self {
            name: name.to_string(),
            command: command.to_string(),
            args: Vec::new(),
            env_names: Vec::new(),
            env_assignments: Vec::new(),
            prompt_args: Vec::new(),
            plan_args: Vec::new(),
            resume_args: Vec::new(),
            startup_input: Vec::new(),
            rename_input: Vec::new(),
        }
    }

    pub fn with_args(self, args: &[String]) -> Self {
        Self {
            args: args.to_vec(),
            ..self
        }
    }

    pub fn with_env_names(self, names: &[String]) -> Self {
        let mut env_names: Vec<String> = Vec::new();
        for name in names {
            if !env_names.contains(name) {
                env_names.push(name.clone());
            }
        }
        Self { env_names, ..self }
    }

    pub fn with_env_assignments(self, assignments: &[(String, String)]) -> Self {
        Self {
            env_assignments: assignments.to_vec(),
            ..self
        }
    }

    pub fn with_prompt_args(self, template: &[String]) -> Self {
        Self {
            prompt_args: template.to_vec(),
            ..self
        }
    }

    pub fn with_plan_args(self, args: &[String]) -> Self {
        Self {
            plan_args: args.to_vec(),
            ..self
        }
    }

    pub fn with_resume_args(self, args: &[String]) -> Self {
        Self {
            resume_args: args.to_vec(),
            ..self
        }
    }

    pub fn with_startup_input(self, lines: &[String]) -> Self {
        Self {
            startup_input: lines.to_vec(),
            ..self
        }
    }

    pub fn with_rename_input(self, lines: &[String]) -> Self {
        Self {
            rename_input: lines.to_vec(),
            ..self
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn env_names(&self) -> &[String] {
        &self.env_names
    }

    pub fn env_assignments(&self) -> &[(String, String)] {
        &self.env_assignments
    }

    pub fn prompt_args(&self) -> &[String] {
        &self.prompt_args
    }

    pub fn accepts_prompt(&self) -> bool {
        self.prompt_args
            .iter()
            .any(|element| element.contains(Self::PROMPT_PLACEHOLDER))
    }

    pub fn launch_refusal(&self, mode: AgentMode, prompt: &str) -> Option<String> {
        self.prompt_refusal(prompt)
            .or_else(|| self.mode_refusal(mode))
    }

    fn prompt_refusal(&self, prompt: &str) -> Option<String> {
        if prompt.is_empty() || self.accepts_prompt() {
            return None;
        }
        Some(format!(
            "agent {} cannot take a prompt, its prompt_args has no {}",
            self.name,
            Self::PROMPT_PLACEHOLDER
        ))
    }

    fn mode_refusal(&self, mode: AgentMode) -> Option<String> {
        if self.supports(mode) {
            return None;
        }
        Some(format!(
            "agent {} cannot start in {} mode, it has no plan_args",
            self.name,
            mode.name()
        ))
    }

    pub fn launch_argv(&self, session: &SessionName, mode: AgentMode, prompt: &str) -> Vec<String> {
        let mut argv = self.restart_argv(session);
        argv.extend_from_slice(self.mode_args(mode));
        if !prompt.is_empty() {
            argv.extend(self.expanded_prompt_args(prompt));
        }
        argv
    }

    pub fn restart_argv(&self, session: &SessionName) -> Vec<String> {
        let mut argv = vec![self.command.clone()];
        argv.extend(Self::named(&self.args, session));
        argv
    }

    pub fn resume_argv(&self, session: &SessionName) -> Vec<String> {
        let mut argv = self.restart_argv(session);
        argv.extend(Self::named(&self.resume_args, session));
        argv
    }

    pub fn startup_input(&self, session: &SessionName) -> Vec<String> {
        Self::named(&self.startup_input, session)
    }

    pub fn rename_input(&self, session: &SessionName) -> Vec<String> {
        Self::named(&self.rename_input, session)
    }

    fn named(template: &[String], session: &SessionName) -> Vec<String> {
        template
            .iter()
            .map(|element| element.replace(Self::NAME_PLACEHOLDER, session.as_str()))
            .collect()
    }

    pub fn supports(&self, mode: AgentMode) -> bool {
        match mode {
            AgentMode::Build => true,
            AgentMode::Plan => !self.plan_args.is_empty(),
        }
    }

    fn mode_args(&self, mode: AgentMode) -> &[String] {
        match mode {
            AgentMode::Build => &[],
            AgentMode::Plan => &self.plan_args,
        }
    }

    fn expanded_prompt_args(&self, prompt: &str) -> impl Iterator<Item = String> {
        self.prompt_args
            .iter()
            .map(move |element| element.replace(Self::PROMPT_PLACEHOLDER, prompt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AgentFixture;

    impl AgentFixture {
        fn with_template(template: &[&str]) -> Agent {
            let args = ["--model".to_string(), "opus".to_string()];
            Agent::new("example-agent", "example-agent")
                .with_args(&args)
                .with_prompt_args(&Self::words(template))
        }

        fn named() -> Agent {
            Agent::new("example-agent", "example-agent")
                .with_args(&Self::words(&["--name", "{name}"]))
                .with_prompt_args(&Self::words(&["{prompt}"]))
                .with_startup_input(&Self::words(&["/color", "hello {name}"]))
                .with_rename_input(&Self::words(&["/rename {name}"]))
        }

        fn planning() -> Agent {
            Self::with_template(&["{prompt}"]).with_plan_args(&Self::words(&["--mode", "plan"]))
        }

        fn resuming() -> Agent {
            Self::named().with_resume_args(&Self::words(&["--continue", "--title={name}"]))
        }

        fn session() -> SessionName {
            SessionName::new("fix-auth").unwrap()
        }

        fn words(parts: &[&str]) -> Vec<String> {
            parts.iter().map(|part| part.to_string()).collect()
        }
    }

    #[test]
    fn the_argv_is_command_then_args_then_prompt_args() {
        let agent = AgentFixture::with_template(&["{prompt}"]);

        let argv = agent.launch_argv(&AgentFixture::session(), AgentMode::Build, "fix the tests");

        assert_eq!(argv, ["example-agent", "--model", "opus", "fix the tests"]);
    }

    #[test]
    fn plan_mode_puts_the_plan_args_between_the_args_and_the_prompt_args() {
        let agent = AgentFixture::planning();

        let argv = agent.launch_argv(&AgentFixture::session(), AgentMode::Plan, "fix the tests");

        assert_eq!(
            argv,
            [
                "example-agent",
                "--model",
                "opus",
                "--mode",
                "plan",
                "fix the tests"
            ]
        );
    }

    #[test]
    fn build_mode_leaves_out_the_plan_args() {
        let agent = AgentFixture::planning();

        let argv = agent.launch_argv(&AgentFixture::session(), AgentMode::Build, "fix the tests");

        assert_eq!(argv, ["example-agent", "--model", "opus", "fix the tests"]);
    }

    #[test]
    fn plan_mode_for_an_agent_without_plan_args_is_refused() {
        let agent = AgentFixture::with_template(&["{prompt}"]);

        assert_eq!(
            agent.launch_refusal(AgentMode::Plan, "").as_deref(),
            Some("agent example-agent cannot start in plan mode, it has no plan_args")
        );
    }

    #[test]
    fn plan_mode_for_an_agent_with_plan_args_is_never_refused() {
        assert_eq!(
            AgentFixture::planning().launch_refusal(AgentMode::Plan, ""),
            None
        );
    }

    #[test]
    fn build_mode_is_never_refused() {
        let agent = AgentFixture::with_template(&[]);

        assert_eq!(agent.launch_refusal(AgentMode::Build, ""), None);
    }

    #[test]
    fn an_empty_prompt_appends_no_prompt_args() {
        let agent = AgentFixture::with_template(&["--prompt", "{prompt}"]);

        let argv = agent.launch_argv(&AgentFixture::session(), AgentMode::Build, "");

        assert_eq!(argv, ["example-agent", "--model", "opus"]);
    }

    #[test]
    fn a_multi_word_prompt_stays_one_argv_element() {
        let agent = AgentFixture::with_template(&["{prompt}"]);

        let argv = agent.launch_argv(
            &AgentFixture::session(),
            AgentMode::Build,
            "fix \"the\" tests;",
        );

        assert_eq!(argv.last().unwrap(), "fix \"the\" tests;");
    }

    #[test]
    fn the_placeholder_expands_inside_a_larger_element() {
        let agent = AgentFixture::with_template(&["--prompt={prompt}"]);

        let argv = agent.launch_argv(
            &AgentFixture::session(),
            AgentMode::Build,
            "- fix the tests",
        );

        assert_eq!(argv.last().unwrap(), "--prompt=- fix the tests");
    }

    #[test]
    fn the_placeholder_expands_twice_in_one_element() {
        let agent = AgentFixture::with_template(&["{prompt}|{prompt}"]);

        let argv = agent.launch_argv(&AgentFixture::session(), AgentMode::Build, "go");

        assert_eq!(argv.last().unwrap(), "go|go");
    }

    #[test]
    fn a_prompt_containing_the_placeholder_is_not_expanded_again() {
        let agent = AgentFixture::with_template(&["--prompt={prompt}"]);

        let argv = agent.launch_argv(&AgentFixture::session(), AgentMode::Build, "say {prompt}");

        assert_eq!(argv.last().unwrap(), "--prompt=say {prompt}");
    }

    #[test]
    fn restart_keeps_args_and_never_replays_a_prompt() {
        let agent = AgentFixture::with_template(&["{prompt}"]);

        assert_eq!(
            agent.restart_argv(&AgentFixture::session()),
            ["example-agent", "--model", "opus"]
        );
    }

    #[test]
    fn the_name_placeholder_expands_in_args_on_launch() {
        let argv = AgentFixture::named().launch_argv(
            &AgentFixture::session(),
            AgentMode::Build,
            "fix the tests",
        );

        assert_eq!(
            argv,
            ["example-agent", "--name", "fix-auth", "fix the tests"]
        );
    }

    #[test]
    fn the_name_placeholder_expands_in_args_on_restart() {
        let argv = AgentFixture::named().restart_argv(&AgentFixture::session());

        assert_eq!(argv, ["example-agent", "--name", "fix-auth"]);
    }

    #[test]
    fn resume_puts_the_resume_args_after_the_args_with_the_name_expanded() {
        let argv = AgentFixture::resuming().resume_argv(&AgentFixture::session());

        assert_eq!(
            argv,
            [
                "example-agent",
                "--name",
                "fix-auth",
                "--continue",
                "--title=fix-auth"
            ]
        );
    }

    #[test]
    fn restart_leaves_out_the_resume_args() {
        let argv = AgentFixture::resuming().restart_argv(&AgentFixture::session());

        assert_eq!(argv, ["example-agent", "--name", "fix-auth"]);
    }

    #[test]
    fn launch_leaves_out_the_resume_args() {
        let argv = AgentFixture::resuming().launch_argv(
            &AgentFixture::session(),
            AgentMode::Build,
            "fix the tests",
        );

        assert_eq!(
            argv,
            ["example-agent", "--name", "fix-auth", "fix the tests"]
        );
    }

    #[test]
    fn resume_without_resume_args_is_a_restart() {
        let argv = AgentFixture::named().resume_argv(&AgentFixture::session());

        assert_eq!(argv, ["example-agent", "--name", "fix-auth"]);
    }

    #[test]
    fn a_prompt_containing_the_name_placeholder_is_not_expanded() {
        let argv = AgentFixture::named().launch_argv(
            &AgentFixture::session(),
            AgentMode::Build,
            "say {name}",
        );

        assert_eq!(argv.last().unwrap(), "say {name}");
    }

    #[test]
    fn the_startup_input_carries_the_session_name() {
        let lines = AgentFixture::named().startup_input(&AgentFixture::session());

        assert_eq!(lines, ["/color", "hello fix-auth"]);
    }

    #[test]
    fn the_rename_input_carries_the_session_name() {
        let lines = AgentFixture::named().rename_input(&AgentFixture::session());

        assert_eq!(lines, ["/rename fix-auth"]);
    }

    #[test]
    fn an_agent_without_input_has_no_lines() {
        let agent = AgentFixture::with_template(&["{prompt}"]);

        assert!(agent.startup_input(&AgentFixture::session()).is_empty());
        assert!(agent.rename_input(&AgentFixture::session()).is_empty());
    }

    #[test]
    fn a_template_without_the_placeholder_cannot_carry_a_prompt() {
        let agent = AgentFixture::with_template(&["--yes"]);

        assert!(!agent.accepts_prompt());
    }

    #[test]
    fn an_empty_template_cannot_carry_a_prompt() {
        let agent = AgentFixture::with_template(&[]);

        assert!(!agent.accepts_prompt());
    }

    #[test]
    fn a_prompt_for_an_agent_without_the_placeholder_is_refused() {
        let agent = AgentFixture::with_template(&["--yes"]);

        assert_eq!(
            agent
                .launch_refusal(AgentMode::Build, "fix the tests")
                .as_deref(),
            Some("agent example-agent cannot take a prompt, its prompt_args has no {prompt}")
        );
    }

    #[test]
    fn an_empty_prompt_is_never_refused() {
        let agent = AgentFixture::with_template(&[]);

        assert_eq!(agent.launch_refusal(AgentMode::Build, ""), None);
    }

    #[test]
    fn locale_and_xdg_variables_belong_to_the_baseline() {
        assert!(Agent::is_baseline_variable("LC_PAPER"));
        assert!(Agent::is_baseline_variable("XDG_RUNTIME_DIR"));
        assert!(!Agent::is_baseline_variable("GPG_TTY"));
    }

    #[test]
    fn a_name_listed_twice_in_env_counts_once() {
        let names = ["API_KEY".to_string(), "API_KEY".to_string()];

        let agent = Agent::new("example-agent", "example-agent").with_env_names(&names);

        assert_eq!(agent.env_names(), ["API_KEY"]);
    }
}

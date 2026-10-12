use super::session_name::SessionName;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    name: String,
    command: String,
    args: Vec<String>,
    env_names: Vec<String>,
    env_assignments: Vec<(String, String)>,
    prompt_args: Vec<String>,
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

    pub fn prompt_refusal(&self, prompt: &str) -> Option<String> {
        if prompt.is_empty() || self.accepts_prompt() {
            return None;
        }
        Some(format!(
            "agent {} cannot take a prompt, its prompt_args has no {}",
            self.name,
            Self::PROMPT_PLACEHOLDER
        ))
    }

    pub fn launch_argv(&self, session: &SessionName, prompt: &str) -> Vec<String> {
        let mut argv = self.restart_argv(session);
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

        let argv = agent.launch_argv(&AgentFixture::session(), "fix the tests");

        assert_eq!(argv, ["example-agent", "--model", "opus", "fix the tests"]);
    }

    #[test]
    fn an_empty_prompt_appends_no_prompt_args() {
        let agent = AgentFixture::with_template(&["--prompt", "{prompt}"]);

        let argv = agent.launch_argv(&AgentFixture::session(), "");

        assert_eq!(argv, ["example-agent", "--model", "opus"]);
    }

    #[test]
    fn a_multi_word_prompt_stays_one_argv_element() {
        let agent = AgentFixture::with_template(&["{prompt}"]);

        let argv = agent.launch_argv(&AgentFixture::session(), "fix \"the\" tests;");

        assert_eq!(argv.last().unwrap(), "fix \"the\" tests;");
    }

    #[test]
    fn the_placeholder_expands_inside_a_larger_element() {
        let agent = AgentFixture::with_template(&["--prompt={prompt}"]);

        let argv = agent.launch_argv(&AgentFixture::session(), "- fix the tests");

        assert_eq!(argv.last().unwrap(), "--prompt=- fix the tests");
    }

    #[test]
    fn the_placeholder_expands_twice_in_one_element() {
        let agent = AgentFixture::with_template(&["{prompt}|{prompt}"]);

        let argv = agent.launch_argv(&AgentFixture::session(), "go");

        assert_eq!(argv.last().unwrap(), "go|go");
    }

    #[test]
    fn a_prompt_containing_the_placeholder_is_not_expanded_again() {
        let agent = AgentFixture::with_template(&["--prompt={prompt}"]);

        let argv = agent.launch_argv(&AgentFixture::session(), "say {prompt}");

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
        let argv = AgentFixture::named().launch_argv(&AgentFixture::session(), "fix the tests");

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
    fn a_prompt_containing_the_name_placeholder_is_not_expanded() {
        let argv = AgentFixture::named().launch_argv(&AgentFixture::session(), "say {name}");

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
            agent.prompt_refusal("fix the tests").as_deref(),
            Some("agent example-agent cannot take a prompt, its prompt_args has no {prompt}")
        );
    }

    #[test]
    fn an_empty_prompt_is_never_refused() {
        let agent = AgentFixture::with_template(&[]);

        assert_eq!(agent.prompt_refusal(""), None);
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

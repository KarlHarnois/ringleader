use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use serde_json::Value;

use super::bounded_command::{BoundedCommand, Call};
use super::command_output::CommandOutput;
use super::deadlines::CallKind;
use super::null_directory_environment::NullDirectoryEnvironment;
use super::process_environment::ProcessEnvironment;
use super::shell_word::ShellWord;
use crate::application::ports::{
    DirectoryEnvironment, DirectoryEnvironmentError, Envrc, HostProbe,
};
use crate::domain::TerminalText;

pub struct DirenvDirectoryEnvironment {
    program: OsString,
    baseline: ProcessEnvironment,
    bounded: BoundedCommand,
}

impl DirenvDirectoryEnvironment {
    const PROGRAM: &'static str = "direnv";
    const ENVRC: &'static str = ".envrc";
    const STATUS: &'static str = "status";
    const JSON: &'static str = "--json";
    const ALLOW: &'static str = "allow";
    const ALLOWED_STATUS: u64 = 0;
    const DENIED_STATUS: u64 = 2;

    pub fn new(program: &OsStr, environment: &ProcessEnvironment, bounded: BoundedCommand) -> Self {
        Self {
            program: program.to_os_string(),
            baseline: environment.baseline(),
            bounded,
        }
    }

    pub fn when_installed(
        host: &dyn HostProbe,
        environment: &ProcessEnvironment,
        bounded: BoundedCommand,
    ) -> Arc<dyn DirectoryEnvironment> {
        match host.resolve_command(Self::PROGRAM) {
            Some(program) => Arc::new(Self::new(program.as_os_str(), environment, bounded)),
            None => Arc::new(NullDirectoryEnvironment),
        }
    }

    fn allow_status(&self, directory: &Path) -> Option<u64> {
        let mut status = self.direnv(directory);
        status.args([Self::STATUS, Self::JSON]);
        self.bounded
            .run(status, &Call::new(Self::STATUS, CallKind::Direnv))
            .ok()
            .filter(CommandOutput::succeeded)
            .and_then(|output| Self::reported_allow_status(&output.stdout_text()))
    }

    fn reported_allow_status(status: &str) -> Option<u64> {
        serde_json::from_str::<Value>(status)
            .ok()
            .and_then(|status| status["state"]["foundRC"]["allowed"].as_u64())
    }

    fn direnv(&self, directory: &Path) -> Command {
        let mut command = Command::new(&self.program);
        command
            .current_dir(directory)
            .env_clear()
            .envs(self.baseline.variables().iter().cloned());
        command
    }

    fn allow_command(directory: &Path) -> String {
        let directory = directory.display().to_string();
        format!(
            "{} {} {}",
            Self::PROGRAM,
            Self::ALLOW,
            ShellWord::new(&directory)
        )
    }

    fn error_text(output: &CommandOutput) -> String {
        TerminalText::new(&output.stderr_text()).plain()
    }

    fn allow_failure(directory: &Path, reason: String) -> DirectoryEnvironmentError {
        DirectoryEnvironmentError::AllowFailed {
            directory: directory.to_path_buf(),
            reason,
        }
    }
}

impl DirectoryEnvironment for DirenvDirectoryEnvironment {
    fn envrc(&self, directory: &Path) -> Option<Envrc> {
        let path = directory.join(Self::ENVRC);
        if !path.is_file() {
            return None;
        }
        let envrc = Envrc::new(fs::read(&path).ok(), &Self::allow_command(directory));
        match self.allow_status(directory) {
            Some(Self::ALLOWED_STATUS) => Some(envrc.allowed()),
            Some(Self::DENIED_STATUS) => Some(envrc.denied()),
            _ => Some(envrc),
        }
    }

    fn allow(&self, directory: &Path) -> Result<(), DirectoryEnvironmentError> {
        let mut allow = self.direnv(directory);
        allow.arg(Self::ALLOW).arg(directory);
        let output = self
            .bounded
            .run(allow, &Call::new(Self::ALLOW, CallKind::Direnv))
            .map_err(|error| Self::allow_failure(directory, error.to_string()))?;
        match output.succeeded() {
            true => Ok(()),
            false => Err(Self::allow_failure(directory, Self::error_text(&output))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::Deadlines;
    use crate::test_support::{DirenvTestHome, FakeHostProbe};

    struct DirenvFixture {
        home: DirenvTestHome,
    }

    impl DirenvFixture {
        const ENVRC: &'static str = "export EXAMPLE_FROM_ENVRC=loaded\n";

        fn new() -> Self {
            Self {
                home: DirenvTestHome::new(),
            }
        }

        fn direnv(&self) -> DirenvDirectoryEnvironment {
            self.direnv_in(&self.home.environment())
        }

        fn direnv_in(&self, environment: &ProcessEnvironment) -> DirenvDirectoryEnvironment {
            DirenvDirectoryEnvironment::new(
                &DirenvTestHome::program(),
                environment,
                BoundedCommand::new(Deadlines::standard()),
            )
        }

        fn whitelisted_by_direnv_config(&self, project: &Path) -> ProcessEnvironment {
            let mut variables = self.home.variables();
            variables.push(self.home.config_variable_whitelisting(project));
            ProcessEnvironment::new(variables, None)
        }

        fn project_with_envrc(&self) -> std::path::PathBuf {
            self.home.project_with_envrc("example-project", Self::ENVRC)
        }

        fn on_host(&self, host: FakeHostProbe) -> Arc<dyn DirectoryEnvironment> {
            DirenvDirectoryEnvironment::when_installed(
                &host,
                &self.home.environment(),
                BoundedCommand::new(Deadlines::standard()),
            )
        }
    }

    #[test]
    fn a_host_without_direnv_finds_no_envrc() {
        let fixture = DirenvFixture::new();
        let project = fixture.project_with_envrc();

        let environment = fixture.on_host(FakeHostProbe::default());

        assert_eq!(environment.envrc(&project), None);
    }

    #[test]
    fn a_host_with_direnv_finds_the_envrc() {
        let fixture = DirenvFixture::new();
        let project = fixture.project_with_envrc();

        let environment = fixture.on_host(FakeHostProbe::default().with_command("direnv"));

        assert!(environment.envrc(&project).is_some());
    }

    #[test]
    fn a_directory_without_an_envrc_has_none() {
        let fixture = DirenvFixture::new();
        let project = fixture.home.project("example-project");

        assert_eq!(fixture.direnv().envrc(&project), None);
    }

    #[test]
    fn a_new_envrc_is_read_and_not_allowed() {
        let fixture = DirenvFixture::new();
        let project = fixture.project_with_envrc();

        let envrc = fixture.direnv().envrc(&project).unwrap();

        assert!(!envrc.is_allowed());
        let same = Envrc::new(Some(DirenvFixture::ENVRC.as_bytes().to_vec()), "");
        assert!(envrc.has_contents_of(&same));
    }

    #[test]
    fn an_envrc_the_user_allowed_reads_as_allowed() {
        let fixture = DirenvFixture::new();
        let project = fixture.project_with_envrc();
        fixture.home.allow_as_the_user(&project);

        assert!(fixture.direnv().envrc(&project).unwrap().is_allowed());
    }

    #[test]
    fn an_envrc_the_user_denied_reads_as_denied() {
        let fixture = DirenvFixture::new();
        let project = fixture.project_with_envrc();
        fixture.home.deny_as_the_user(&project);

        assert!(fixture.direnv().envrc(&project).unwrap().is_denied());
    }

    #[test]
    fn a_whitelist_in_a_direnv_config_the_agent_never_sees_does_not_allow_the_envrc() {
        let fixture = DirenvFixture::new();
        let project = fixture.project_with_envrc();
        let environment = fixture.whitelisted_by_direnv_config(&project);

        let envrc = fixture.direnv_in(&environment).envrc(&project).unwrap();

        assert!(!envrc.is_allowed());
    }

    #[test]
    fn an_allowed_envrc_reads_as_allowed_and_joins_the_allow_list() {
        let fixture = DirenvFixture::new();
        let project = fixture.project_with_envrc();

        fixture.direnv().allow(&project).unwrap();

        assert!(fixture.direnv().envrc(&project).unwrap().is_allowed());
        assert_eq!(fs::read_dir(fixture.home.allow_list()).unwrap().count(), 1);
    }

    #[test]
    fn a_failed_allow_carries_the_direnv_error_without_its_colors() {
        let fixture = DirenvFixture::new();
        let project = fixture.home.project("example-project");

        let error = fixture.direnv().allow(&project).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "cannot allow the .envrc of {}: direnv: error .envrc file not found",
                project.display()
            )
        );
    }

    #[test]
    fn the_allow_command_names_the_directory() {
        let fixture = DirenvFixture::new();
        let project = fixture.project_with_envrc();

        let envrc = fixture.direnv().envrc(&project).unwrap();

        assert_eq!(
            envrc.allow_command(),
            format!("direnv allow {}", project.display())
        );
    }

    #[test]
    fn the_allow_command_quotes_a_directory_with_a_space() {
        let fixture = DirenvFixture::new();
        let project = fixture
            .home
            .project_with_envrc("example project", DirenvFixture::ENVRC);

        let envrc = fixture.direnv().envrc(&project).unwrap();

        assert_eq!(
            envrc.allow_command(),
            format!("direnv allow '{}'", project.display())
        );
    }

    #[test]
    fn a_status_reports_the_allow_status_of_the_found_envrc() {
        let blocked = r#"{"state": {"foundRC": {"allowed": 1, "path": "/repos/.envrc"}}}"#;

        assert_eq!(
            DirenvDirectoryEnvironment::reported_allow_status(blocked),
            Some(1)
        );
    }

    #[test]
    fn a_status_that_is_not_json_reports_no_allow_status() {
        assert_eq!(
            DirenvDirectoryEnvironment::reported_allow_status("not json"),
            None
        );
    }
}

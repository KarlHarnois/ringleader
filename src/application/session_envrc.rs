use std::path::Path;
use std::sync::Arc;

use super::ports::{AgentLaunch, DirectoryEnvironment, Envrc};
use super::service_ports::ServicePorts;

pub struct SessionEnvrc {
    environment: Arc<dyn DirectoryEnvironment>,
}

impl SessionEnvrc {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            environment: ports.repos().directory_environment(),
        }
    }

    pub fn choose(&self, worktree: &Path, main_checkout: &Path) -> EnvrcChoice {
        let Some(envrc) = self.environment.envrc(worktree) else {
            return EnvrcChoice::Skip;
        };
        if envrc.is_allowed() {
            return EnvrcChoice::Load;
        }
        if self.main_checkout_vouches_for(main_checkout, &envrc) {
            return self.allow(worktree, main_checkout);
        }
        Self::not_allowed(worktree, &envrc)
    }

    fn main_checkout_vouches_for(&self, main_checkout: &Path, envrc: &Envrc) -> bool {
        self.environment
            .envrc(main_checkout)
            .is_some_and(|main| main.is_allowed() && envrc.has_contents_of(&main))
    }

    fn allow(&self, worktree: &Path, main_checkout: &Path) -> EnvrcChoice {
        match self.environment.allow(worktree) {
            Ok(()) => {
                log::info!(
                    "allowed the .envrc of {}, which matches the allowed .envrc of {}",
                    worktree.display(),
                    main_checkout.display()
                );
                EnvrcChoice::Load
            }
            Err(error) => {
                EnvrcChoice::Warn(format!("the agent started without its .envrc, {error}"))
            }
        }
    }

    fn not_allowed(worktree: &Path, envrc: &Envrc) -> EnvrcChoice {
        EnvrcChoice::Warn(format!(
            "the .envrc of {} is not allowed, so the agent started without it, review the .envrc and run {}",
            worktree.display(),
            envrc.allow_command()
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvrcChoice {
    Skip,
    Load,
    Warn(String),
}

impl EnvrcChoice {
    pub fn applied_to(&self, launch: AgentLaunch) -> AgentLaunch {
        match self {
            Self::Load => launch.loading_envrc(),
            Self::Skip | Self::Warn(_) => launch,
        }
    }

    pub fn warning(&self) -> Option<String> {
        match self {
            Self::Warn(warning) => Some(warning.clone()),
            Self::Skip | Self::Load => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::test_support::{FakeDirectoryEnvironment, ServiceFixture};

    struct EnvrcFixture;

    impl EnvrcFixture {
        const FLAKE: &'static str = "use flake\n";

        fn worktree() -> PathBuf {
            ServiceFixture::worktree_path("fix-auth")
        }

        fn main_checkout() -> &'static Path {
            Path::new(ServiceFixture::REPO)
        }

        fn with_main_checkout(contents: &str) -> FakeDirectoryEnvironment {
            FakeDirectoryEnvironment::default().with_allowed_envrc(Self::main_checkout(), contents)
        }

        fn choose(environment: FakeDirectoryEnvironment) -> (ServiceFixture, EnvrcChoice) {
            let fixture = ServiceFixture::new().with_directory_environment(environment);
            let choice = SessionEnvrc::new(&fixture.ports())
                .choose(&Self::worktree(), Self::main_checkout());
            (fixture, choice)
        }

        fn launch() -> AgentLaunch {
            AgentLaunch::new(vec!["claude".to_string()], &[], &Self::worktree())
        }
    }

    #[test]
    fn a_worktree_without_an_envrc_starts_plainly_without_a_warning() {
        let (_, choice) =
            EnvrcFixture::choose(EnvrcFixture::with_main_checkout(EnvrcFixture::FLAKE));

        assert_eq!(choice, EnvrcChoice::Skip);
    }

    #[test]
    fn an_allowed_envrc_is_loaded_without_allowing_it_again() {
        let environment = FakeDirectoryEnvironment::default()
            .with_allowed_envrc(&EnvrcFixture::worktree(), EnvrcFixture::FLAKE);

        let (fixture, choice) = EnvrcFixture::choose(environment);

        assert_eq!(choice, EnvrcChoice::Load);
        assert!(fixture.directory_environment().allowed().is_empty());
    }

    #[test]
    fn an_envrc_identical_to_the_allowed_main_checkout_one_is_allowed_then_loaded() {
        let environment = EnvrcFixture::with_main_checkout(EnvrcFixture::FLAKE)
            .with_envrc(&EnvrcFixture::worktree(), EnvrcFixture::FLAKE);

        let (fixture, choice) = EnvrcFixture::choose(environment);

        assert_eq!(choice, EnvrcChoice::Load);
        assert_eq!(
            fixture.directory_environment().allowed(),
            [EnvrcFixture::worktree()]
        );
    }

    #[test]
    fn an_envrc_one_byte_away_from_the_main_checkout_one_is_never_allowed() {
        let environment = EnvrcFixture::with_main_checkout(EnvrcFixture::FLAKE)
            .with_envrc(&EnvrcFixture::worktree(), "use flake \n");

        let (fixture, choice) = EnvrcFixture::choose(environment);

        assert!(matches!(choice, EnvrcChoice::Warn(_)));
        assert!(fixture.directory_environment().allowed().is_empty());
    }

    #[test]
    fn an_envrc_is_never_allowed_when_the_main_checkout_has_none() {
        let environment = FakeDirectoryEnvironment::default()
            .with_envrc(&EnvrcFixture::worktree(), EnvrcFixture::FLAKE);

        let (fixture, choice) = EnvrcFixture::choose(environment);

        assert!(matches!(choice, EnvrcChoice::Warn(_)));
        assert!(fixture.directory_environment().allowed().is_empty());
    }

    #[test]
    fn an_envrc_identical_to_a_main_checkout_one_that_is_not_allowed_is_never_allowed() {
        let environment = FakeDirectoryEnvironment::default()
            .with_envrc(EnvrcFixture::main_checkout(), EnvrcFixture::FLAKE)
            .with_envrc(&EnvrcFixture::worktree(), EnvrcFixture::FLAKE);

        let (fixture, choice) = EnvrcFixture::choose(environment);

        assert!(matches!(choice, EnvrcChoice::Warn(_)));
        assert!(fixture.directory_environment().allowed().is_empty());
    }

    #[test]
    fn the_not_allowed_warning_names_the_worktree_and_the_allow_command() {
        let environment = FakeDirectoryEnvironment::default()
            .with_envrc(&EnvrcFixture::worktree(), EnvrcFixture::FLAKE);
        let worktree = EnvrcFixture::worktree().display().to_string();

        let (_, choice) = EnvrcFixture::choose(environment);

        assert_eq!(
            choice.warning().unwrap(),
            format!(
                "the .envrc of {worktree} is not allowed, so the agent started without it, review the .envrc and run direnv allow {worktree}"
            )
        );
    }

    #[test]
    fn a_failed_allow_starts_plainly_and_warns_with_the_error() {
        let environment = EnvrcFixture::with_main_checkout(EnvrcFixture::FLAKE)
            .with_envrc(&EnvrcFixture::worktree(), EnvrcFixture::FLAKE)
            .failing_allow("direnv: error permission denied");

        let (_, choice) = EnvrcFixture::choose(environment);

        assert_eq!(
            EnvrcFixture::launch(),
            choice.applied_to(EnvrcFixture::launch())
        );
        assert!(
            choice
                .warning()
                .unwrap()
                .contains("direnv: error permission denied")
        );
    }

    #[test]
    fn a_load_marks_the_launch_to_load_the_envrc() {
        let launch = EnvrcChoice::Load.applied_to(EnvrcFixture::launch());

        assert!(launch.loads_envrc());
    }

    #[test]
    fn a_warning_keeps_the_plain_launch() {
        let choice = EnvrcChoice::Warn("not allowed".to_string());

        assert!(!choice.applied_to(EnvrcFixture::launch()).loads_envrc());
    }
}

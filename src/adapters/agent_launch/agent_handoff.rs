use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use super::agent_variables::AgentVariables;
use super::built_in_skill::BuiltInSkill;
use super::handoff_contents::HandoffContents;
use super::ringleader_shim::RingleaderShim;
use crate::adapters::config_file::toml_config_source::TomlConfigSource;
use crate::adapters::host::process_environment::ProcessEnvironment;
use crate::adapters::host::process_table::ProcessTable;
use crate::application::ports::AgentLaunch;

static HANDOFF_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct AgentHandoff {
    directory: PathBuf,
    environment: ProcessEnvironment,
    processes: ProcessTable,
    skill: Option<BuiltInSkill>,
    shim: Option<RingleaderShim>,
    config_path: Option<PathBuf>,
}

impl AgentHandoff {
    pub const STALE_AFTER: Duration = Duration::from_secs(60);
    const DIRECTORY_MODE: u32 = 0o700;
    const FILE_MODE: u32 = 0o600;
    const ROOT: &'static str = "/";
    const LAUNCH_PREFIX: &'static str = "handoff";
    const RELAY_PREFIX: &'static str = "relay";
    const NAME_SEPARATOR: char = '-';

    pub fn new(
        directory: &Path,
        environment: &ProcessEnvironment,
        processes: ProcessTable,
    ) -> Self {
        Self {
            directory: directory.to_path_buf(),
            environment: environment.clone(),
            processes,
            skill: None,
            shim: None,
            config_path: None,
        }
    }

    pub fn with_shim(self, shim: RingleaderShim) -> Self {
        Self {
            shim: Some(shim),
            ..self
        }
    }

    pub fn with_built_in_skill(self, skill: BuiltInSkill) -> Self {
        Self {
            skill: Some(skill),
            ..self
        }
    }

    pub fn with_config_path(self, config_path: &Path) -> Self {
        Self {
            config_path: Some(config_path.to_path_buf()),
            ..self
        }
    }

    pub fn beside(
        handoff: &Path,
        environment: &ProcessEnvironment,
        processes: ProcessTable,
    ) -> Self {
        let directory = handoff.parent().unwrap_or(Path::new(Self::ROOT));
        Self::new(directory, environment, processes)
    }

    pub fn write(&self, launch: &AgentLaunch) -> io::Result<PathBuf> {
        let variables = AgentVariables::new(
            &self.environment,
            launch.env_names(),
            &self.assignments_for(launch),
        );
        Self::warn_about_missing(variables.missing());
        let contents =
            HandoffContents::for_launch(launch, variables.inherited(), variables.explicit());
        let contents = self.offering_skill(contents, launch);
        let contents = self.offering_shim(contents);
        self.write_file(Self::LAUNCH_PREFIX, &contents)
    }

    fn assignments_for(&self, launch: &AgentLaunch) -> Vec<(String, String)> {
        let assigns_its_own = launch
            .env_assignments()
            .iter()
            .any(|(name, _)| name == TomlConfigSource::PATH_VARIABLE);
        let config = self
            .config_path
            .iter()
            .filter(|_| !assigns_its_own)
            .map(|path| {
                (
                    TomlConfigSource::PATH_VARIABLE.to_string(),
                    path.to_string_lossy().into_owned(),
                )
            });
        config
            .chain(launch.env_assignments().iter().cloned())
            .collect()
    }

    fn offering_shim(&self, contents: HandoffContents) -> HandoffContents {
        match self.shim.as_ref().map(RingleaderShim::publish) {
            Some(Ok(folder)) => contents.with_shim_directory(&folder),
            Some(Err(error)) => {
                log::warn!(
                    "could not link ringleader for the agent, it keeps the PATH it inherits: {error}"
                );
                contents
            }
            None => contents,
        }
    }

    fn offering_skill(&self, contents: HandoffContents, launch: &AgentLaunch) -> HandoffContents {
        let skill = self
            .skill
            .as_ref()
            .filter(|_| launch.offers_built_in_skills());
        match skill.map(BuiltInSkill::publish) {
            Some(Ok(directory)) => contents.with_skill_directory(&directory),
            Some(Err(error)) => {
                log::warn!(
                    "could not publish the built-in skill, the agent starts without it: {error}"
                );
                contents
            }
            None => contents,
        }
    }

    fn warn_about_missing(names: &[String]) {
        for name in names {
            log::warn!("the agent variable {name} is not set, the agent starts without it");
        }
    }

    pub fn write_relay(&self, contents: &HandoffContents) -> io::Result<PathBuf> {
        self.write_file(Self::RELAY_PREFIX, contents)
    }

    fn write_file(&self, prefix: &str, contents: &HandoffContents) -> io::Result<PathBuf> {
        DirBuilder::new()
            .recursive(true)
            .mode(Self::DIRECTORY_MODE)
            .create(&self.directory)?;
        self.delete_stale();
        let path = self.next_path(prefix);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(Self::FILE_MODE)
            .open(&path)?;
        file.write_all(&serde_json::to_vec(contents).map_err(io::Error::other)?)?;
        file.sync_all()?;
        Ok(path)
    }

    pub fn delete(path: &Path) {
        if let Err(error) = fs::remove_file(path)
            && error.kind() != io::ErrorKind::NotFound
        {
            log::warn!(
                "could not delete the handoff file {}: {error}",
                path.display()
            );
        }
    }

    pub fn delete_stale(&self) {
        let Ok(entries) = fs::read_dir(&self.directory) else {
            return;
        };
        let now = SystemTime::now();
        for entry in entries.filter_map(Result::ok) {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let age = metadata
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok());
            let stale = metadata.is_file() && age.is_some_and(|age| age > Self::STALE_AFTER);
            if stale && !self.relay_writer_runs(&entry.path()) {
                Self::delete(&entry.path());
            }
        }
    }

    fn relay_writer_runs(&self, path: &Path) -> bool {
        Self::relay_writer(path).is_some_and(|writer| self.processes.is_alive(writer))
    }

    fn relay_writer(path: &Path) -> Option<u32> {
        let name = path.file_name()?.to_str()?;
        let numbers = name
            .strip_prefix(Self::RELAY_PREFIX)?
            .strip_prefix(Self::NAME_SEPARATOR)?;
        numbers.split(Self::NAME_SEPARATOR).next()?.parse().ok()
    }

    pub fn take(path: &Path) -> io::Result<HandoffContents> {
        let bytes = fs::read(path);
        Self::delete(path);
        serde_json::from_slice(&bytes?).map_err(io::Error::other)
    }

    fn next_path(&self, prefix: &str) -> PathBuf {
        let sequence = HANDOFF_COUNTER.fetch_add(1, Ordering::Relaxed);
        let separator = Self::NAME_SEPARATOR;
        let writer = std::process::id();
        self.directory.join(format!(
            "{prefix}{separator}{writer}{separator}{sequence}.json"
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs::File;

    use super::*;
    use crate::adapters::agent_launch::ringleader_shim::RingleaderShim;
    use crate::test_support::ProcessMarker;

    struct HandoffFixture {
        directory: tempfile::TempDir,
    }

    impl HandoffFixture {
        const STALE: Duration = Duration::from_secs(61);
        const NO_SUCH_PROCESS: u32 = u32::MAX;

        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn handoff_directory(&self) -> PathBuf {
            self.directory.path().join("run/ringleader/handoff")
        }

        fn handoff(&self, pairs: &[(&str, &str)]) -> AgentHandoff {
            let variables = pairs
                .iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value)))
                .collect();
            AgentHandoff::new(
                &self.handoff_directory(),
                &ProcessEnvironment::new(variables, None),
                ProcessMarker::new().table(),
            )
        }

        fn launch(env_names: &[&str]) -> AgentLaunch {
            let names: Vec<String> = env_names.iter().map(|name| name.to_string()).collect();
            AgentLaunch::new(
                vec!["claude".to_string(), "fix it".to_string()],
                &names,
                Path::new("/worktrees/fix-auth"),
            )
        }

        fn launch_setting(name: &str, value: &str) -> AgentLaunch {
            Self::launch(&[]).with_env_assignments(&[(name.to_string(), value.to_string())])
        }

        fn plugin_root(&self) -> PathBuf {
            self.handoff_directory().with_file_name("plugin")
        }

        fn skilled_handoff(&self, plugin_root: &Path) -> AgentHandoff {
            self.handoff(&[])
                .with_built_in_skill(BuiltInSkill::new(plugin_root))
        }

        fn offering() -> AgentLaunch {
            Self::launch(&[]).offering_built_in_skills()
        }

        fn written(handoff: &AgentHandoff, launch: &AgentLaunch) -> HandoffContents {
            AgentHandoff::take(&handoff.write(launch).unwrap()).unwrap()
        }

        fn handed(&self, pairs: &[(&str, &str)], launch: &AgentLaunch) -> HandoffContents {
            let path = self.handoff(pairs).write(launch).unwrap();
            AgentHandoff::take(&path).unwrap()
        }

        fn pair(name: &str, value: &str) -> (OsString, OsString) {
            (OsString::from(name), OsString::from(value))
        }

        fn aged_file(&self, name: &str, age: Duration) -> PathBuf {
            fs::create_dir_all(self.handoff_directory()).unwrap();
            let path = self.handoff_directory().join(name);
            File::create(&path).unwrap();
            Self::age(&path, age);
            path
        }

        fn age(path: &Path, age: Duration) {
            let file = File::options().write(true).open(path).unwrap();
            file.set_modified(SystemTime::now() - age).unwrap();
        }

        fn stale_launch_handoff(&self) -> PathBuf {
            let path = self.handoff(&[]).write(&Self::launch(&[])).unwrap();
            Self::age(&path, Self::STALE);
            path
        }

        fn stale_relay(&self) -> PathBuf {
            let contents = self.handed(&[], &Self::launch(&[])).inside_envrc();
            let path = self.handoff(&[]).write_relay(&contents).unwrap();
            Self::age(&path, Self::STALE);
            path
        }

        fn relay_of_an_ended_launcher(&self) -> PathBuf {
            let name = format!(
                "{}-{}-0.json",
                AgentHandoff::RELAY_PREFIX,
                Self::NO_SUCH_PROCESS
            );
            self.aged_file(&name, Self::STALE)
        }
    }

    #[test]
    fn a_handoff_carries_the_argv_the_worktree_and_the_baseline_and_configured_values() {
        let fixture = HandoffFixture::new();
        let pairs = [("PATH", "/bin"), ("API_KEY", "secret"), ("UNRELATED", "x")];

        let contents = fixture.handed(&pairs, &HandoffFixture::launch(&["API_KEY"]));

        assert_eq!(contents.argv(), ["claude", "fix it"]);
        assert_eq!(contents.worktree(), Path::new("/worktrees/fix-auth"));
        assert_eq!(
            contents.inherited(),
            [
                HandoffFixture::pair("PATH", "/bin"),
                HandoffFixture::pair("API_KEY", "secret")
            ]
        );
    }

    #[test]
    fn a_configured_value_is_also_explicit() {
        let fixture = HandoffFixture::new();
        let pairs = [("PATH", "/bin"), ("API_KEY", "secret")];

        let contents = fixture.handed(&pairs, &HandoffFixture::launch(&["API_KEY"]));

        assert_eq!(
            contents.explicit(),
            [HandoffFixture::pair("API_KEY", "secret")]
        );
    }

    #[test]
    fn a_set_value_is_explicit_and_never_inherited() {
        let fixture = HandoffFixture::new();
        let launch = HandoffFixture::launch_setting("EXAMPLE_ACCOUNT_DIR", "/accounts/example");

        let contents = fixture.handed(&[("PATH", "/bin")], &launch);

        assert_eq!(contents.inherited(), [HandoffFixture::pair("PATH", "/bin")]);
        assert_eq!(
            contents.explicit(),
            [HandoffFixture::pair(
                "EXAMPLE_ACCOUNT_DIR",
                "/accounts/example"
            )]
        );
    }

    #[test]
    fn the_shell_ringleader_started_with_is_explicit() {
        let fixture = HandoffFixture::new();
        let pairs = [("PATH", "/bin"), ("SHELL", "/bin/example-shell")];

        let contents = fixture.handed(&pairs, &HandoffFixture::launch(&[]));

        assert_eq!(
            contents.explicit(),
            [HandoffFixture::pair("SHELL", "/bin/example-shell")]
        );
    }

    #[test]
    fn a_set_shell_replaces_the_one_ringleader_started_with() {
        let fixture = HandoffFixture::new();
        let launch = HandoffFixture::launch_setting("SHELL", "/bin/set-shell");

        let contents = fixture.handed(&[("SHELL", "/bin/example-shell")], &launch);

        assert_eq!(
            contents.explicit(),
            [HandoffFixture::pair("SHELL", "/bin/set-shell")]
        );
    }

    #[test]
    fn a_set_value_replaces_a_configured_one_among_the_explicit_values() {
        let fixture = HandoffFixture::new();
        let launch = HandoffFixture::launch(&["API_KEY"])
            .with_env_assignments(&[("API_KEY".to_string(), "assigned".to_string())]);

        let contents = fixture.handed(&[("API_KEY", "inherited")], &launch);

        assert_eq!(
            contents.explicit(),
            [HandoffFixture::pair("API_KEY", "assigned")]
        );
    }

    #[test]
    fn a_handoff_beside_another_writes_into_its_directory() {
        let fixture = HandoffFixture::new();
        let first = fixture
            .handoff(&[])
            .write(&HandoffFixture::launch(&[]))
            .unwrap();
        let contents = AgentHandoff::take(&first).unwrap();

        let second = AgentHandoff::beside(
            &first,
            &ProcessEnvironment::default(),
            ProcessMarker::new().table(),
        )
        .write_relay(&contents.inside_envrc())
        .unwrap();

        assert_eq!(second.parent(), Some(fixture.handoff_directory().as_path()));
    }

    #[test]
    fn taking_a_handoff_deletes_the_file() {
        let fixture = HandoffFixture::new();
        let path = fixture
            .handoff(&[])
            .write(&HandoffFixture::launch(&[]))
            .unwrap();

        AgentHandoff::take(&path).unwrap();

        assert!(!path.exists());
    }

    #[test]
    fn the_handoff_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = HandoffFixture::new();

        let path = fixture
            .handoff(&[])
            .write(&HandoffFixture::launch(&[]))
            .unwrap();

        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn a_write_recreates_a_deleted_handoff_directory() {
        let fixture = HandoffFixture::new();
        let handoff = fixture.handoff(&[]);
        handoff.write(&HandoffFixture::launch(&[])).unwrap();
        fs::remove_dir_all(fixture.handoff_directory()).unwrap();

        assert!(handoff.write(&HandoffFixture::launch(&[])).is_ok());
    }

    #[test]
    fn a_file_older_than_sixty_seconds_is_deleted() {
        let fixture = HandoffFixture::new();
        let stale = fixture.aged_file("handoff-old.json", Duration::from_secs(61));

        fixture.handoff(&[]).delete_stale();

        assert!(!stale.exists());
    }

    #[test]
    fn a_fresh_file_is_kept() {
        let fixture = HandoffFixture::new();
        let fresh = fixture.aged_file("handoff-new.json", Duration::from_secs(5));

        fixture.handoff(&[]).delete_stale();

        assert!(fresh.exists());
    }

    #[test]
    fn a_stale_relay_is_kept_while_the_launcher_that_wrote_it_runs() {
        let fixture = HandoffFixture::new();
        let relay = fixture.stale_relay();

        fixture.handoff(&[]).delete_stale();

        assert!(relay.exists());
    }

    #[test]
    fn a_stale_relay_is_deleted_once_the_launcher_that_wrote_it_ended() {
        let fixture = HandoffFixture::new();
        let relay = fixture.relay_of_an_ended_launcher();

        fixture.handoff(&[]).delete_stale();

        assert!(!relay.exists());
    }

    #[test]
    fn a_stale_launch_handoff_is_deleted_while_its_writer_runs() {
        let fixture = HandoffFixture::new();
        let handoff = fixture.stale_launch_handoff();

        fixture.handoff(&[]).delete_stale();

        assert!(!handoff.exists());
    }

    #[test]
    fn a_missing_directory_has_nothing_stale() {
        let fixture = HandoffFixture::new();

        fixture.handoff(&[]).delete_stale();

        assert!(!fixture.handoff_directory().exists());
    }

    #[test]
    fn a_launch_that_offers_the_skill_names_the_published_folder() {
        let fixture = HandoffFixture::new();
        let handoff = fixture.skilled_handoff(&fixture.plugin_root());

        let contents = HandoffFixture::written(&handoff, &HandoffFixture::offering());

        assert!(
            contents
                .skill_directory()
                .is_some_and(|folder| folder.is_dir())
        );
    }

    #[test]
    fn a_launch_that_offers_no_skill_names_no_folder() {
        let fixture = HandoffFixture::new();
        let handoff = fixture.skilled_handoff(&fixture.plugin_root());

        let contents = HandoffFixture::written(&handoff, &HandoffFixture::launch(&[]));

        assert_eq!(contents.skill_directory(), None);
    }

    #[test]
    fn a_skill_that_cannot_be_published_leaves_the_launch_without_it() {
        let fixture = HandoffFixture::new();
        let blocker = fixture.handoff_directory().with_file_name("blocker");
        fs::create_dir_all(blocker.parent().unwrap()).unwrap();
        fs::write(&blocker, "").unwrap();
        let handoff = fixture.skilled_handoff(&blocker.join("plugin"));

        let contents = HandoffFixture::written(&handoff, &HandoffFixture::offering());

        assert_eq!(contents.skill_directory(), None);
    }

    #[test]
    fn the_agent_gets_the_config_path_of_the_ringleader_that_launches_it() {
        let fixture = HandoffFixture::new();
        let handoff = fixture
            .handoff(&[])
            .with_config_path(Path::new("/config/example.toml"));

        let contents = HandoffFixture::written(&handoff, &HandoffFixture::launch(&[]));

        assert_eq!(
            contents.explicit(),
            [HandoffFixture::pair(
                "RINGLEADER_CONFIG",
                "/config/example.toml"
            )]
        );
    }

    #[test]
    fn a_config_path_the_agent_sets_itself_wins() {
        let fixture = HandoffFixture::new();
        let handoff = fixture
            .handoff(&[])
            .with_config_path(Path::new("/config/example.toml"));
        let launch = HandoffFixture::launch_setting("RINGLEADER_CONFIG", "/config/own.toml");

        let contents = HandoffFixture::written(&handoff, &launch);

        assert_eq!(
            contents.explicit(),
            [HandoffFixture::pair(
                "RINGLEADER_CONFIG",
                "/config/own.toml"
            )]
        );
    }

    #[test]
    fn a_handoff_names_the_folder_that_links_the_launching_ringleader() {
        let fixture = HandoffFixture::new();
        let shim = RingleaderShim::new(
            &fixture.handoff_directory().with_file_name("bin"),
            Path::new("/builds/example/ringleader"),
        );
        let handoff = fixture.handoff(&[]).with_shim(shim);

        let contents = HandoffFixture::written(&handoff, &HandoffFixture::launch(&[]));

        let folder = contents.shim_directory().unwrap();
        assert_eq!(
            fs::read_link(folder.join("ringleader")).unwrap(),
            Path::new("/builds/example/ringleader")
        );
    }

    #[test]
    fn a_shim_that_cannot_be_linked_leaves_the_launch_without_it() {
        let fixture = HandoffFixture::new();
        let blocker = fixture.handoff_directory().with_file_name("blocker");
        fs::create_dir_all(blocker.parent().unwrap()).unwrap();
        fs::write(&blocker, "").unwrap();
        let shim = RingleaderShim::new(
            &blocker.join("bin"),
            Path::new("/builds/example/ringleader"),
        );
        let handoff = fixture.handoff(&[]).with_shim(shim);

        let contents = HandoffFixture::written(&handoff, &HandoffFixture::launch(&[]));

        assert_eq!(contents.shim_directory(), None);
    }
}

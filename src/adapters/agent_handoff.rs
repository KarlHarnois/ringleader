use std::ffi::OsString;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use super::handoff_contents::HandoffContents;
use super::live_process::LiveProcess;
use super::process_environment::ProcessEnvironment;
use crate::application::ports::AgentLaunch;

static HANDOFF_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct AgentHandoff {
    directory: PathBuf,
    environment: ProcessEnvironment,
}

impl AgentHandoff {
    pub const STALE_AFTER: Duration = Duration::from_secs(60);
    const DIRECTORY_MODE: u32 = 0o700;
    const FILE_MODE: u32 = 0o600;
    const ROOT: &'static str = "/";
    const LAUNCH_PREFIX: &'static str = "handoff";
    const RELAY_PREFIX: &'static str = "relay";
    const NAME_SEPARATOR: char = '-';

    pub fn new(directory: &Path, environment: &ProcessEnvironment) -> Self {
        Self {
            directory: directory.to_path_buf(),
            environment: environment.clone(),
        }
    }

    pub fn beside(handoff: &Path, environment: &ProcessEnvironment) -> Self {
        let directory = handoff.parent().unwrap_or(Path::new(Self::ROOT));
        Self::new(directory, environment)
    }

    pub fn write(&self, launch: &AgentLaunch) -> io::Result<PathBuf> {
        let named = self.named_variables(launch);
        let inherited = [self.baseline_variables(), named.clone()].concat();
        let explicit = Self::overridden(named, Self::assigned_variables(launch));
        let contents = HandoffContents::for_launch(launch, &inherited, &explicit);
        self.write_file(Self::LAUNCH_PREFIX, &contents)
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
            if stale && !Self::relay_writer_runs(&entry.path()) {
                Self::delete(&entry.path());
            }
        }
    }

    fn relay_writer_runs(path: &Path) -> bool {
        Self::relay_writer(path)
            .and_then(LiveProcess::read)
            .is_some_and(|writer| writer.is_alive())
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

    fn overridden(
        variables: Vec<(OsString, OsString)>,
        overrides: Vec<(OsString, OsString)>,
    ) -> Vec<(OsString, OsString)> {
        let is_overridden =
            |name: &OsString| overrides.iter().any(|(overriding, _)| overriding == name);
        let mut combined: Vec<(OsString, OsString)> = variables
            .into_iter()
            .filter(|(name, _)| !is_overridden(name))
            .collect();
        combined.extend(overrides);
        combined
    }

    fn baseline_variables(&self) -> Vec<(OsString, OsString)> {
        self.environment.baseline().variables().to_vec()
    }

    fn named_variables(&self, launch: &AgentLaunch) -> Vec<(OsString, OsString)> {
        let mut named = Vec::new();
        for name in launch.env_names() {
            match self.environment.get(name) {
                Some(value) => named.push((OsString::from(name), value.to_os_string())),
                None => {
                    log::warn!("the agent variable {name} is not set, the agent starts without it")
                }
            }
        }
        named
    }

    fn assigned_variables(launch: &AgentLaunch) -> Vec<(OsString, OsString)> {
        launch
            .env_assignments()
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect()
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
    use std::fs::File;

    use super::*;

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

        let second = AgentHandoff::beside(&first, &ProcessEnvironment::default())
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
}

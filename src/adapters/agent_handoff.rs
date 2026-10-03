use std::ffi::{OsStr, OsString};
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use super::process_environment::ProcessEnvironment;
use crate::application::ports::AgentLaunch;
use crate::domain::Agent;

static HANDOFF_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct AgentHandoff {
    directory: PathBuf,
    environment: ProcessEnvironment,
}

impl AgentHandoff {
    pub const STALE_AFTER: Duration = Duration::from_secs(60);
    const DIRECTORY_MODE: u32 = 0o700;
    const FILE_MODE: u32 = 0o600;
    pub fn new(directory: &Path, environment: &ProcessEnvironment) -> Self {
        Self {
            directory: directory.to_path_buf(),
            environment: environment.clone(),
        }
    }

    pub fn write(&self, launch: &AgentLaunch) -> io::Result<PathBuf> {
        DirBuilder::new()
            .recursive(true)
            .mode(Self::DIRECTORY_MODE)
            .create(&self.directory)?;
        self.delete_stale();
        let path = self.next_path();
        let contents = HandoffContents::for_launch(launch, &self.handed_environment(launch));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(Self::FILE_MODE)
            .open(&path)?;
        file.write_all(&serde_json::to_vec(&contents).map_err(io::Error::other)?)?;
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
            if metadata.is_file() && age.is_some_and(|age| age > Self::STALE_AFTER) {
                Self::delete(&entry.path());
            }
        }
    }

    pub fn take(path: &Path) -> io::Result<HandoffContents> {
        let bytes = fs::read(path);
        Self::delete(path);
        serde_json::from_slice(&bytes?).map_err(io::Error::other)
    }

    fn handed_environment(&self, launch: &AgentLaunch) -> Vec<(OsString, OsString)> {
        let mut handed: Vec<(OsString, OsString)> = self
            .environment
            .variables()
            .iter()
            .filter(|(name, _)| Agent::is_baseline_variable(&name.to_string_lossy()))
            .cloned()
            .collect();
        for name in launch.env_names() {
            match self.environment.get(name) {
                Some(value) => handed.push((OsString::from(name), value.to_os_string())),
                None => {
                    log::warn!("the agent variable {name} is not set, the agent starts without it")
                }
            }
        }
        handed
    }

    fn next_path(&self) -> PathBuf {
        let sequence = HANDOFF_COUNTER.fetch_add(1, Ordering::Relaxed);
        self.directory
            .join(format!("handoff-{}-{sequence}.json", std::process::id()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffContents {
    argv: Vec<Vec<u8>>,
    environment: Vec<(Vec<u8>, Vec<u8>)>,
    worktree: Vec<u8>,
}

impl HandoffContents {
    fn for_launch(launch: &AgentLaunch, environment: &[(OsString, OsString)]) -> Self {
        Self {
            argv: launch
                .argv()
                .iter()
                .map(|word| word.as_bytes().to_vec())
                .collect(),
            environment: environment
                .iter()
                .map(|(name, value)| (name.as_bytes().to_vec(), value.as_bytes().to_vec()))
                .collect(),
            worktree: launch.worktree().as_os_str().as_bytes().to_vec(),
        }
    }

    pub fn argv(&self) -> Vec<OsString> {
        self.argv
            .iter()
            .map(|word| OsString::from_vec(word.clone()))
            .collect()
    }

    pub fn environment(&self) -> Vec<(OsString, OsString)> {
        self.environment
            .iter()
            .map(|(name, value)| {
                (
                    OsString::from_vec(name.clone()),
                    OsString::from_vec(value.clone()),
                )
            })
            .collect()
    }

    pub fn worktree(&self) -> PathBuf {
        PathBuf::from(OsStr::from_bytes(&self.worktree))
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

        fn aged_file(&self, name: &str, age: Duration) -> PathBuf {
            fs::create_dir_all(self.handoff_directory()).unwrap();
            let path = self.handoff_directory().join(name);
            let file = File::create(&path).unwrap();
            file.set_modified(SystemTime::now() - age).unwrap();
            path
        }
    }

    #[test]
    fn a_handoff_carries_the_argv_the_worktree_and_the_baseline_and_configured_values() {
        let fixture = HandoffFixture::new();
        let handoff =
            fixture.handoff(&[("PATH", "/bin"), ("API_KEY", "secret"), ("UNRELATED", "x")]);

        let path = handoff
            .write(&HandoffFixture::launch(&["API_KEY"]))
            .unwrap();
        let contents = AgentHandoff::take(&path).unwrap();

        assert_eq!(contents.argv(), ["claude", "fix it"]);
        assert_eq!(contents.worktree(), Path::new("/worktrees/fix-auth"));
        assert_eq!(
            contents.environment(),
            [
                ("PATH".into(), "/bin".into()),
                ("API_KEY".into(), "secret".into())
            ]
        );
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
    fn a_missing_directory_has_nothing_stale() {
        let fixture = HandoffFixture::new();

        fixture.handoff(&[]).delete_stale();

        assert!(!fixture.handoff_directory().exists());
    }
}

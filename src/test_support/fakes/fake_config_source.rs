use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::application::ports::{ConfigError, ConfigSource};
use crate::domain::{Config, Repo, RepoName};

pub struct FakeConfigSource {
    path: PathBuf,
    state: Mutex<ConfigState>,
}

struct ConfigState {
    config: Config,
    load_error: Option<ConfigError>,
    write_error: Option<ConfigError>,
    writes: usize,
}

impl FakeConfigSource {
    pub const PATH: &'static str = "/config/ringleader/config.toml";

    pub fn with_config(config: Config) -> Self {
        Self {
            path: PathBuf::from(Self::PATH),
            state: Mutex::new(ConfigState {
                config,
                load_error: None,
                write_error: None,
                writes: 0,
            }),
        }
    }

    pub fn replace(&self, config: Config) {
        let mut state = self.state.lock().unwrap();
        state.config = config;
        state.load_error = None;
    }

    pub fn break_with(&self, error: ConfigError) {
        self.state.lock().unwrap().load_error = Some(error);
    }

    pub fn config(&self) -> Config {
        self.state.lock().unwrap().config.clone()
    }

    pub fn write_count(&self) -> usize {
        self.state.lock().unwrap().writes
    }

    fn write(
        &self,
        edit: impl FnOnce(&Config) -> Result<Config, ConfigError>,
    ) -> Result<(), ConfigError> {
        let mut state = self.state.lock().unwrap();
        if let Some(error) = &state.write_error {
            return Err(error.clone());
        }
        state.config = edit(&state.config)?;
        state.writes += 1;
        Ok(())
    }

    fn unknown_repo(&self, name: &RepoName) -> ConfigError {
        ConfigError::UnknownRepo {
            path: self.path.clone(),
            name: name.to_string(),
        }
    }

    fn path_of(&self) -> &Path {
        &self.path
    }
}

impl ConfigSource for FakeConfigSource {
    fn path(&self) -> PathBuf {
        self.path_of().to_path_buf()
    }

    fn load(&self) -> Result<Config, ConfigError> {
        let state = self.state.lock().unwrap();
        match &state.load_error {
            Some(error) => Err(error.clone()),
            None => Ok(state.config.clone()),
        }
    }

    fn add_repo(&self, repo: &Repo) -> Result<(), ConfigError> {
        self.write(|config| {
            let mut repos = config.repos().to_vec();
            repos.push(repo.clone());
            Ok(config.clone().with_repos(repos))
        })
    }

    fn remove_repo(&self, name: &RepoName) -> Result<(), ConfigError> {
        let unknown = self.unknown_repo(name);
        self.write(|config| {
            let repos: Vec<Repo> = config
                .repos()
                .iter()
                .filter(|repo| repo.name() != name)
                .cloned()
                .collect();
            if repos.len() == config.repos().len() {
                return Err(unknown);
            }
            Ok(config.clone().with_repos(repos))
        })
    }
}

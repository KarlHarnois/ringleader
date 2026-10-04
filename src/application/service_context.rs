use std::path::{Path, PathBuf};

use super::diagnostic_paths::DiagnosticPaths;
use super::ports::ConfigError;
use crate::domain::Config;

pub struct ServiceContext {
    config: LoadedConfig,
    diagnostics: DiagnosticPaths,
    inside_tmux: bool,
    launch_directory: PathBuf,
    home: Option<PathBuf>,
    shell: String,
}

impl ServiceContext {
    pub fn new(config: LoadedConfig, launch_directory: &Path) -> Self {
        Self {
            config,
            diagnostics: DiagnosticPaths::default(),
            inside_tmux: false,
            launch_directory: launch_directory.to_path_buf(),
            home: None,
            shell: "sh".to_string(),
        }
    }

    pub fn with_inside_tmux(self, inside_tmux: bool) -> Self {
        Self {
            inside_tmux,
            ..self
        }
    }

    pub fn with_shell(self, shell: &str) -> Self {
        Self {
            shell: shell.to_string(),
            ..self
        }
    }

    pub fn with_diagnostics(self, diagnostics: DiagnosticPaths) -> Self {
        Self {
            diagnostics,
            ..self
        }
    }

    pub fn diagnostics(&self) -> &DiagnosticPaths {
        &self.diagnostics
    }

    pub fn with_home(self, home: Option<&Path>) -> Self {
        Self {
            home: home.map(Path::to_path_buf),
            ..self
        }
    }

    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    pub fn is_inside_tmux(&self) -> bool {
        self.inside_tmux
    }

    pub fn shell(&self) -> &str {
        &self.shell
    }

    pub fn launch_directory(&self) -> &Path {
        &self.launch_directory
    }

    pub fn into_config(self) -> LoadedConfig {
        self.config
    }
}

pub struct LoadedConfig {
    config: Config,
    error: Option<ConfigError>,
}

impl LoadedConfig {
    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn error(&self) -> Option<&ConfigError> {
        self.error.as_ref()
    }

    pub fn valid(config: Config) -> Self {
        Self {
            config,
            error: None,
        }
    }

    pub fn failed(defaults: Config, error: ConfigError) -> Self {
        Self {
            config: defaults,
            error: Some(error),
        }
    }
}

use std::sync::Arc;

use super::ports::{
    Clock, ConfigSource, DirectoryEnvironment, HostProbe, Multiplexer, PullRequests, RepoScanner,
    ScratchSpace, SessionRegistry, TerminalEmulator, Workspace,
};

pub struct ServicePorts {
    state: StatePorts,
    repos: RepoPorts,
    tmux: TmuxPorts,
    desktop: DesktopPorts,
}

impl ServicePorts {
    pub fn new(
        state: StatePorts,
        repos: RepoPorts,
        tmux: TmuxPorts,
        desktop: DesktopPorts,
    ) -> Self {
        Self {
            state,
            repos,
            tmux,
            desktop,
        }
    }

    pub fn state(&self) -> &StatePorts {
        &self.state
    }

    pub fn repos(&self) -> &RepoPorts {
        &self.repos
    }

    pub fn tmux(&self) -> &TmuxPorts {
        &self.tmux
    }

    pub fn desktop(&self) -> &DesktopPorts {
        &self.desktop
    }
}

pub struct StatePorts {
    registry: Arc<dyn SessionRegistry>,
    config_source: Arc<dyn ConfigSource>,
    clock: Arc<dyn Clock>,
    scratch_space: Arc<dyn ScratchSpace>,
}

impl StatePorts {
    pub fn new(
        registry: Arc<dyn SessionRegistry>,
        config_source: Arc<dyn ConfigSource>,
        clock: Arc<dyn Clock>,
        scratch_space: Arc<dyn ScratchSpace>,
    ) -> Self {
        Self {
            registry,
            config_source,
            clock,
            scratch_space,
        }
    }

    pub fn registry(&self) -> Arc<dyn SessionRegistry> {
        Arc::clone(&self.registry)
    }

    pub fn config_source(&self) -> Arc<dyn ConfigSource> {
        Arc::clone(&self.config_source)
    }

    pub fn clock(&self) -> Arc<dyn Clock> {
        Arc::clone(&self.clock)
    }

    pub fn scratch_space(&self) -> Arc<dyn ScratchSpace> {
        Arc::clone(&self.scratch_space)
    }
}

pub struct RepoPorts {
    workspace: Arc<dyn Workspace>,
    diff_workspace: Arc<dyn Workspace>,
    scanner: Arc<dyn RepoScanner>,
    host: Arc<dyn HostProbe>,
    pull_requests: Arc<dyn PullRequests>,
    directory_environment: Arc<dyn DirectoryEnvironment>,
}

impl RepoPorts {
    pub fn new(
        workspace: Arc<dyn Workspace>,
        scanner: Arc<dyn RepoScanner>,
        host: Arc<dyn HostProbe>,
        pull_requests: Arc<dyn PullRequests>,
        directory_environment: Arc<dyn DirectoryEnvironment>,
    ) -> Self {
        Self {
            diff_workspace: Arc::clone(&workspace),
            workspace,
            scanner,
            host,
            pull_requests,
            directory_environment,
        }
    }

    pub fn with_diff_workspace(self, diff_workspace: Arc<dyn Workspace>) -> Self {
        Self {
            diff_workspace,
            ..self
        }
    }

    pub fn workspace(&self) -> Arc<dyn Workspace> {
        Arc::clone(&self.workspace)
    }

    pub fn diff_workspace(&self) -> Arc<dyn Workspace> {
        Arc::clone(&self.diff_workspace)
    }

    pub fn scanner(&self) -> Arc<dyn RepoScanner> {
        Arc::clone(&self.scanner)
    }

    pub fn host(&self) -> Arc<dyn HostProbe> {
        Arc::clone(&self.host)
    }

    pub fn pull_requests(&self) -> Arc<dyn PullRequests> {
        Arc::clone(&self.pull_requests)
    }

    pub fn directory_environment(&self) -> Arc<dyn DirectoryEnvironment> {
        Arc::clone(&self.directory_environment)
    }
}

pub struct TmuxPorts {
    multiplexer: Arc<dyn Multiplexer>,
}

impl TmuxPorts {
    pub fn new(multiplexer: Arc<dyn Multiplexer>) -> Self {
        Self { multiplexer }
    }

    pub fn multiplexer(&self) -> Arc<dyn Multiplexer> {
        Arc::clone(&self.multiplexer)
    }
}

pub struct DesktopPorts {
    terminal_emulator: Arc<dyn TerminalEmulator>,
}

impl DesktopPorts {
    pub fn new(terminal_emulator: Arc<dyn TerminalEmulator>) -> Self {
        Self { terminal_emulator }
    }

    pub fn terminal_emulator(&self) -> Arc<dyn TerminalEmulator> {
        Arc::clone(&self.terminal_emulator)
    }
}

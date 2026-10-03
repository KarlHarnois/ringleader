use std::sync::Arc;

use super::ports::{
    Clock, ConfigSource, HostProbe, Multiplexer, RepoScanner, ReturnBinding, SessionRegistry,
    Workspace,
};

pub struct ServicePorts {
    state: StatePorts,
    repos: RepoPorts,
    tmux: TmuxPorts,
}

impl ServicePorts {
    pub fn new(state: StatePorts, repos: RepoPorts, tmux: TmuxPorts) -> Self {
        Self { state, repos, tmux }
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
}

pub struct StatePorts {
    registry: Arc<dyn SessionRegistry>,
    config_source: Arc<dyn ConfigSource>,
    clock: Arc<dyn Clock>,
}

impl StatePorts {
    pub fn new(
        registry: Arc<dyn SessionRegistry>,
        config_source: Arc<dyn ConfigSource>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            registry,
            config_source,
            clock,
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
}

pub struct RepoPorts {
    workspace: Arc<dyn Workspace>,
    scanner: Arc<dyn RepoScanner>,
    host: Arc<dyn HostProbe>,
}

impl RepoPorts {
    pub fn new(
        workspace: Arc<dyn Workspace>,
        scanner: Arc<dyn RepoScanner>,
        host: Arc<dyn HostProbe>,
    ) -> Self {
        Self {
            workspace,
            scanner,
            host,
        }
    }

    pub fn workspace(&self) -> Arc<dyn Workspace> {
        Arc::clone(&self.workspace)
    }

    pub fn scanner(&self) -> Arc<dyn RepoScanner> {
        Arc::clone(&self.scanner)
    }

    pub fn host(&self) -> Arc<dyn HostProbe> {
        Arc::clone(&self.host)
    }
}

pub struct TmuxPorts {
    multiplexer: Arc<dyn Multiplexer>,
    binding: Arc<dyn ReturnBinding>,
}

impl TmuxPorts {
    pub fn new(multiplexer: Arc<dyn Multiplexer>, binding: Arc<dyn ReturnBinding>) -> Self {
        Self {
            multiplexer,
            binding,
        }
    }

    pub fn multiplexer(&self) -> Arc<dyn Multiplexer> {
        Arc::clone(&self.multiplexer)
    }

    pub fn binding(&self) -> Arc<dyn ReturnBinding> {
        Arc::clone(&self.binding)
    }
}

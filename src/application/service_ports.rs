use std::sync::Arc;

use super::ports::{
    Clock, ConfigSource, HostProbe, Multiplexer, PullRequests, RepoScanner, SessionRegistry,
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
    pull_requests: Arc<dyn PullRequests>,
}

impl RepoPorts {
    pub fn new(
        workspace: Arc<dyn Workspace>,
        scanner: Arc<dyn RepoScanner>,
        host: Arc<dyn HostProbe>,
        pull_requests: Arc<dyn PullRequests>,
    ) -> Self {
        Self {
            workspace,
            scanner,
            host,
            pull_requests,
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

    pub fn pull_requests(&self) -> Arc<dyn PullRequests> {
        Arc::clone(&self.pull_requests)
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

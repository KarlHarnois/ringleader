use std::sync::Arc;

use super::ports::{
    Clock, ConfigSource, DirectoryEnvironment, HostProbe, Multiplexer, PullRequests, RepoPages,
    RepoScanner, ScratchSpace, SessionRegistry, TerminalEmulator, ThemeSource, UsageMeter,
    WebBrowser, Workspace,
};

pub struct ServicePorts {
    state: StatePorts,
    repos: RepoPorts,
    tmux: TmuxPorts,
    desktop: DesktopPorts,
    agents: AgentPorts,
}

impl ServicePorts {
    pub fn new(
        state: StatePorts,
        repos: RepoPorts,
        tmux: TmuxPorts,
        desktop: DesktopPorts,
        agents: AgentPorts,
    ) -> Self {
        Self {
            state,
            repos,
            tmux,
            desktop,
            agents,
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

    pub fn agents(&self) -> &AgentPorts {
        &self.agents
    }
}

pub struct StatePorts {
    registry: Arc<dyn SessionRegistry>,
    config_source: Arc<dyn ConfigSource>,
    theme_source: Arc<dyn ThemeSource>,
    clock: Arc<dyn Clock>,
    scratch_space: Arc<dyn ScratchSpace>,
}

impl StatePorts {
    pub fn new(
        registry: Arc<dyn SessionRegistry>,
        config_source: Arc<dyn ConfigSource>,
        theme_source: Arc<dyn ThemeSource>,
        clock: Arc<dyn Clock>,
        scratch_space: Arc<dyn ScratchSpace>,
    ) -> Self {
        Self {
            registry,
            config_source,
            theme_source,
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

    pub fn theme_source(&self) -> Arc<dyn ThemeSource> {
        Arc::clone(&self.theme_source)
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
    repo_pages: Arc<dyn RepoPages>,
    directory_environment: Arc<dyn DirectoryEnvironment>,
}

impl RepoPorts {
    pub fn new(
        workspace: Arc<dyn Workspace>,
        scanner: Arc<dyn RepoScanner>,
        host: Arc<dyn HostProbe>,
        pull_requests: Arc<dyn PullRequests>,
        repo_pages: Arc<dyn RepoPages>,
        directory_environment: Arc<dyn DirectoryEnvironment>,
    ) -> Self {
        Self {
            diff_workspace: Arc::clone(&workspace),
            workspace,
            scanner,
            host,
            pull_requests,
            repo_pages,
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

    pub fn repo_pages(&self) -> Arc<dyn RepoPages> {
        Arc::clone(&self.repo_pages)
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
    web_browser: Arc<dyn WebBrowser>,
}

impl DesktopPorts {
    pub fn new(
        terminal_emulator: Arc<dyn TerminalEmulator>,
        web_browser: Arc<dyn WebBrowser>,
    ) -> Self {
        Self {
            terminal_emulator,
            web_browser,
        }
    }

    pub fn terminal_emulator(&self) -> Arc<dyn TerminalEmulator> {
        Arc::clone(&self.terminal_emulator)
    }

    pub fn web_browser(&self) -> Arc<dyn WebBrowser> {
        Arc::clone(&self.web_browser)
    }
}

pub struct AgentPorts {
    usage_meters: Vec<Arc<dyn UsageMeter>>,
}

impl AgentPorts {
    pub fn new(usage_meters: Vec<Arc<dyn UsageMeter>>) -> Self {
        Self { usage_meters }
    }

    pub fn usage_meters(&self) -> Vec<Arc<dyn UsageMeter>> {
        self.usage_meters.clone()
    }
}

pub mod agent_launch;
pub mod clock;
pub mod config_source;
pub mod dirty_state;
pub mod host_probe;
pub mod interrupts;
pub mod lock;
pub mod multiplexer;
pub mod pull_requests;
pub mod repo_scanner;
pub mod session_registry;
pub mod unmerged_commits;
pub mod workspace;

pub use agent_launch::AgentLaunch;
pub use clock::Clock;
pub use config_source::{ConfigError, ConfigSource};
pub use dirty_state::DirtyState;
pub use host_probe::{DirectoryAccess, HostProbe, HostProbeError, ProcessInfo};
pub use interrupts::Interrupts;
pub use lock::{Lock, LockError};
pub use multiplexer::{
    AttachTarget, Multiplexer, MultiplexerError, ServerConfig, ServerState, SessionSpec,
};
pub use pull_requests::{PullRequestError, PullRequests};
pub use repo_scanner::{RepoScanner, ScanError, ScanOutcome};
pub use session_registry::{RegistryError, SessionRegistry};
pub use unmerged_commits::UnmergedCommits;
pub use workspace::{StartPoint, Workspace, WorkspaceError, WorktreeEntry};

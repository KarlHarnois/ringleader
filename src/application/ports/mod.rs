pub mod clock;
pub mod config_source;
pub mod host_probe;
pub mod lock;
pub mod multiplexer;
pub mod repo_scanner;
pub mod return_binding;
pub mod session_registry;
pub mod workspace;

pub use clock::Clock;
pub use config_source::{ConfigError, ConfigSource};
pub use host_probe::{HostProbe, HostProbeError, ProcessInfo};
pub use lock::{Lock, LockError};
pub use multiplexer::{AgentLaunch, AttachTarget, Multiplexer, MultiplexerError, SessionSpec};
pub use repo_scanner::{RepoScanner, ScanError, ScanOutcome};
pub use return_binding::{BindingError, ReturnBinding};
pub use session_registry::{RegistryError, SessionRegistry};
pub use workspace::{
    DirtyState, StartPoint, UnmergedCommits, Workspace, WorkspaceError, WorktreeEntry,
};

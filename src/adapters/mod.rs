pub mod agent_launch;
pub mod child_process;
pub mod claude;
pub mod config_file;
pub mod direnv;
pub mod filesystem;
pub mod gh;
pub mod git;
pub mod host;
pub mod ollama;
pub mod opencode;
pub mod sqlite;
pub mod terminal_emulator;
pub mod tmux;
pub mod web_browser;

pub use child_process::bounded_command::BoundedCommand;
pub use child_process::deadlines::Deadlines;
pub use git::git_workspace::GitWorkspace;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("Ringleader builds only for Linux and macOS");

#[cfg(target_os = "macos")]
pub use child_process::kqueue_exit_watch::KqueueExitWatch as ExitWatch;
#[cfg(target_os = "linux")]
pub use child_process::pidfd_exit_watch::PidfdExitWatch as ExitWatch;
#[cfg(target_os = "macos")]
pub use filesystem::getfsstat_mounts::GetfsstatMounts as HostMounts;
#[cfg(target_os = "linux")]
pub use filesystem::mountinfo_file::MountinfoFile as HostMounts;
#[cfg(target_os = "macos")]
pub use host::libproc_process_table::LibprocProcessTable as HostProcessTable;
#[cfg(target_os = "linux")]
pub use host::procfs_process_table::ProcfsProcessTable as HostProcessTable;

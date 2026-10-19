pub mod bounded_command;
pub mod command_output;
pub mod deadlines;
pub mod detached_command;
#[cfg(any(target_os = "macos", test))]
pub mod exit_queue;
#[cfg(target_os = "macos")]
pub mod kqueue_exit_watch;
#[cfg(target_os = "linux")]
pub mod pidfd_exit_watch;

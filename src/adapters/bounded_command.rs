use std::io::{self, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;

use super::command_output::{CommandOutput, PendingOutput};

#[derive(Debug, Clone, Copy)]
pub struct BoundedCommand {
    deadlines: Deadlines,
}

impl BoundedCommand {
    const POLL_INTERVAL: Duration = Duration::from_millis(10);
    const READER_GRACE: Duration = Duration::from_secs(1);
    const TERMINATE_GRACE: Duration = Duration::from_secs(2);
    const KILL_GRACE: Duration = Duration::from_secs(1);

    pub fn new(deadlines: Deadlines) -> Self {
        Self { deadlines }
    }

    pub fn run(&self, command: Command, call: &Call) -> Result<CommandOutput, CommandError> {
        let deadline = self.deadlines.for_kind(call.kind);
        let mut child = self.spawn(command, call)?;
        let output = PendingOutput::collect(&mut child);
        match Self::wait_until(&mut child, Instant::now() + deadline) {
            Some(status) => Ok(output.finish(status, Self::READER_GRACE)),
            None => {
                self.stop(child);
                Err(CommandError::TimedOut {
                    subcommand: call.subcommand.clone(),
                    seconds: deadline.as_secs(),
                })
            }
        }
    }

    fn spawn(&self, mut command: Command, call: &Call) -> Result<Child, CommandError> {
        let stdin = match call.input {
            Some(_) => Stdio::piped(),
            None => Stdio::null(),
        };
        command
            .stdin(stdin)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: the closure runs in the forked child before exec and only calls setsid,
        // which is async-signal-safe and touches no memory shared with the parent.
        unsafe {
            command.pre_exec(|| match libc::setsid() {
                -1 => Err(io::Error::last_os_error()),
                _ => Ok(()),
            });
        }
        let mut child = command.spawn().map_err(|error| CommandError::Spawn {
            subcommand: call.subcommand.clone(),
            detail: error.to_string(),
        })?;
        if let (Some(input), Some(mut stdin)) = (&call.input, child.stdin.take()) {
            let _ = stdin.write_all(input);
        }
        Ok(child)
    }

    fn wait_until(child: &mut Child, deadline: Instant) -> Option<ExitStatus> {
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                return Some(status);
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Self::POLL_INTERVAL);
        }
    }

    fn stop(&self, mut child: Child) {
        self.signal_group(&child, libc::SIGTERM);
        if Self::wait_until(&mut child, Instant::now() + Self::TERMINATE_GRACE).is_some() {
            return;
        }
        self.signal_group(&child, libc::SIGKILL);
        if Self::wait_until(&mut child, Instant::now() + Self::KILL_GRACE).is_some() {
            return;
        }
        thread::spawn(move || child.wait());
    }

    fn signal_group(&self, child: &Child, signal: libc::c_int) {
        let Ok(group) = libc::pid_t::try_from(child.id()) else {
            return;
        };
        // SAFETY: killpg only sends a signal; the group is the child's own session,
        // which setsid created, so no unrelated process group can be addressed.
        unsafe {
            libc::killpg(group, signal);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallKind {
    TmuxQuery,
    TmuxChange,
    GitDiff,
    GitRead,
    GitWrite,
}

#[derive(Debug, Clone, Copy)]
pub struct Deadlines {
    tmux_query: Duration,
    tmux_change: Duration,
    git_diff: Duration,
    git_read: Duration,
    git_write: Duration,
}

impl Deadlines {
    pub fn standard() -> Self {
        Self {
            tmux_query: Duration::from_secs(2),
            tmux_change: Duration::from_secs(10),
            git_diff: Duration::from_secs(5),
            git_read: Duration::from_secs(60),
            git_write: Duration::from_secs(300),
        }
    }

    #[cfg(test)]
    pub fn uniform(deadline: Duration) -> Self {
        Self {
            tmux_query: deadline,
            tmux_change: deadline,
            git_diff: deadline,
            git_read: deadline,
            git_write: deadline,
        }
    }

    fn for_kind(&self, kind: CallKind) -> Duration {
        match kind {
            CallKind::TmuxQuery => self.tmux_query,
            CallKind::TmuxChange => self.tmux_change,
            CallKind::GitDiff => self.git_diff,
            CallKind::GitRead => self.git_read,
            CallKind::GitWrite => self.git_write,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Call {
    subcommand: String,
    kind: CallKind,
    input: Option<Vec<u8>>,
}

impl Call {
    pub fn new(subcommand: &str, kind: CallKind) -> Self {
        Self {
            subcommand: subcommand.to_string(),
            kind,
            input: None,
        }
    }

    pub fn with_input(self, input: &[u8]) -> Self {
        Self {
            input: Some(input.to_vec()),
            ..self
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CommandError {
    #[error("{subcommand} could not start: {detail}")]
    Spawn { subcommand: String, detail: String },
    #[error("{subcommand} timed out after {seconds} seconds")]
    TimedOut { subcommand: String, seconds: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ChildFixture;

    impl ChildFixture {
        fn bounded(deadline: Duration) -> BoundedCommand {
            BoundedCommand::new(Deadlines::uniform(deadline))
        }

        fn shell(script: &str) -> Command {
            let mut command = Command::new("sh");
            command.arg("-c").arg(script);
            command
        }

        fn call() -> Call {
            Call::new("sh", CallKind::GitRead)
        }

        fn session_of_this_process() -> String {
            let stat = std::fs::read_to_string("/proc/self/stat").unwrap();
            Self::session_field(&stat)
        }

        fn session_field(stat: &str) -> String {
            let after_name = &stat[stat.rfind(')').unwrap() + 2..];
            after_name.split(' ').nth(3).unwrap().to_string()
        }
    }

    #[test]
    fn the_child_runs_in_its_own_session() {
        let output = ChildFixture::bounded(Duration::from_secs(5))
            .run(
                ChildFixture::shell("cat /proc/$$/stat"),
                &ChildFixture::call(),
            )
            .unwrap();

        let child_session = ChildFixture::session_field(&output.stdout_text());
        assert_ne!(child_session, ChildFixture::session_of_this_process());
    }

    #[test]
    fn the_input_reaches_the_child_and_stdin_closes() {
        let call = ChildFixture::call().with_input(b"bind-key -T root C-q\n");

        let output = ChildFixture::bounded(Duration::from_secs(5))
            .run(ChildFixture::shell("cat"), &call)
            .unwrap();

        assert_eq!(output.stdout_text(), "bind-key -T root C-q\n");
    }

    #[test]
    fn a_child_without_input_reads_end_of_file() {
        let output = ChildFixture::bounded(Duration::from_secs(5))
            .run(ChildFixture::shell("cat; echo done"), &ChildFixture::call())
            .unwrap();

        assert_eq!(output.stdout_text(), "done\n");
    }

    #[test]
    fn a_child_past_its_deadline_times_out_within_four_seconds() {
        let started = Instant::now();

        let result = ChildFixture::bounded(Duration::from_secs(1)).run(
            ChildFixture::shell("sleep 30 & wait"),
            &ChildFixture::call(),
        );

        assert!(matches!(result, Err(CommandError::TimedOut { .. })));
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn a_background_process_holding_the_pipes_does_not_hold_the_call() {
        let started = Instant::now();

        let output = ChildFixture::bounded(Duration::from_secs(10))
            .run(ChildFixture::shell("sleep 5 &"), &ChildFixture::call())
            .unwrap();

        assert!(output.succeeded());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn a_missing_program_fails_to_spawn() {
        let result = ChildFixture::bounded(Duration::from_secs(1))
            .run(Command::new("/nonexistent/program"), &ChildFixture::call());

        assert!(matches!(result, Err(CommandError::Spawn { .. })));
    }
}

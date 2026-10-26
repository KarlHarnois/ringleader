use std::io::{self, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;

use super::command_output::{CommandOutput, PendingOutput};
use super::deadlines::{CallKind, Deadlines};
use crate::adapters::ExitWatch;
use crate::adapters::host::errno::Errno;

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
        Self::start_in_own_session(&mut command);
        let mut child = command.spawn().map_err(|error| CommandError::Spawn {
            subcommand: call.subcommand.clone(),
            detail: error.to_string(),
        })?;
        if let (Some(input), Some(mut stdin)) = (&call.input, child.stdin.take()) {
            let _ = stdin.write_all(input);
        }
        Ok(child)
    }

    pub fn start_in_own_session(command: &mut Command) {
        // SAFETY: the closure runs in the forked child before exec and only calls setsid,
        // which is async-signal-safe and touches no memory shared with the parent.
        unsafe {
            command.pre_exec(|| Errno::result(libc::setsid()).map(drop));
        }
    }

    fn wait_until(child: &mut Child, deadline: Instant) -> Option<ExitStatus> {
        match ExitWatch::block_until_exit(child, deadline) {
            Ok(()) => Self::reap_once_reapable(child, deadline),
            Err(error) => Self::check_without_exit_watch(child, deadline, &error),
        }
    }

    fn reap_once_reapable(child: &mut Child, deadline: Instant) -> Option<ExitStatus> {
        Self::check_until(child, deadline)
    }

    fn check_without_exit_watch(
        child: &mut Child,
        deadline: Instant,
        error: &io::Error,
    ) -> Option<ExitStatus> {
        log::debug!(
            "could not watch child {} for its exit, checking it every {}ms: {error}",
            child.id(),
            Self::POLL_INTERVAL.as_millis()
        );
        Self::check_until(child, deadline)
    }

    fn check_until(child: &mut Child, deadline: Instant) -> Option<ExitStatus> {
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

        fn reap(mut child: Child) {
            let _ = child.kill();
            let _ = child.wait();
        }

        fn call() -> Call {
            Call::new("sh", CallKind::GitRead)
        }
    }

    #[test]
    fn the_child_runs_in_its_own_session() {
        let output = ChildFixture::bounded(Duration::from_secs(5))
            .run(
                ChildFixture::shell("kill -s 0 -- -$$"),
                &ChildFixture::call(),
            )
            .unwrap();

        assert!(output.succeeded(), "{}", output.stderr_text());
    }

    #[test]
    fn a_program_that_cannot_lead_a_new_session_does_not_start() {
        let mut command = Command::new("true");
        command.process_group(0);
        BoundedCommand::start_in_own_session(&mut command);

        let started = command.spawn().map(|mut child| child.wait());

        assert_eq!(
            started.err().and_then(|error| error.raw_os_error()),
            Some(libc::EPERM)
        );
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
    fn the_fallback_check_sees_an_exited_child() {
        let mut child = ChildFixture::shell("exit 0").spawn().unwrap();

        let status =
            BoundedCommand::check_until(&mut child, Instant::now() + Duration::from_secs(5));

        assert!(status.is_some());
    }

    #[test]
    fn the_fallback_check_gives_up_at_the_deadline() {
        let mut child = ChildFixture::shell("exec sleep 5").spawn().unwrap();

        let status =
            BoundedCommand::check_until(&mut child, Instant::now() + Duration::from_millis(50));
        ChildFixture::reap(child);

        assert!(status.is_none());
    }

    #[test]
    fn a_missing_program_fails_to_spawn() {
        let result = ChildFixture::bounded(Duration::from_secs(1))
            .run(Command::new("/nonexistent/program"), &ChildFixture::call());

        assert!(matches!(result, Err(CommandError::Spawn { .. })));
    }
}

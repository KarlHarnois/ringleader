use std::io;
use std::time::{Duration, Instant};

pub trait ExitQueue {
    fn child(&self) -> u32;
    fn register(&self) -> io::Result<()>;
    fn peek(&self) -> io::Result<WaitReport>;
    fn block_for(&self, timeout: Duration) -> io::Result<()>;

    fn wait_for_exit(&self, deadline: Instant) -> io::Result<()> {
        if self.exited_before_registration()? || self.reports_exit() {
            return Ok(());
        }
        self.block_for(deadline.saturating_duration_since(Instant::now()))
    }

    fn exited_before_registration(&self) -> io::Result<bool> {
        match self.register() {
            Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Ok(true),
            registered => registered.map(|()| false),
        }
    }

    fn reports_exit(&self) -> bool {
        self.peek()
            .is_ok_and(|report| report.shows_exit_of(self.child()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitReport {
    pid: libc::pid_t,
    code: libc::c_int,
}

impl WaitReport {
    const EXIT_CODES: [libc::c_int; 3] = [libc::CLD_EXITED, libc::CLD_KILLED, libc::CLD_DUMPED];

    pub fn new(pid: libc::pid_t, code: libc::c_int) -> Self {
        Self { pid, code }
    }

    fn shows_exit_of(self, child: u32) -> bool {
        u32::try_from(self.pid) == Ok(child) && Self::EXIT_CODES.contains(&self.code)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct ScriptedQueue {
        registration: Option<libc::c_int>,
        report: Result<WaitReport, libc::c_int>,
        calls: RefCell<Vec<&'static str>>,
        timeout: RefCell<Option<Duration>>,
    }

    impl ScriptedQueue {
        const CHILD: u32 = 4100;
        const OTHER: libc::pid_t = 4200;

        fn reporting(report: Result<WaitReport, libc::c_int>) -> Self {
            Self {
                registration: None,
                report,
                calls: RefCell::new(Vec::new()),
                timeout: RefCell::new(None),
            }
        }

        fn running() -> Self {
            Self::reporting(Ok(WaitReport::new(0, 0)))
        }

        fn failing_registration_with(self, errno: libc::c_int) -> Self {
            Self {
                registration: Some(errno),
                ..self
            }
        }

        fn child_report(code: libc::c_int) -> Result<WaitReport, libc::c_int> {
            Ok(WaitReport::new(Self::CHILD as libc::pid_t, code))
        }

        fn timeout(&self) -> Option<Duration> {
            *self.timeout.borrow()
        }

        fn calls(&self) -> Vec<&'static str> {
            self.calls.borrow().clone()
        }

        fn record(&self, call: &'static str) {
            self.calls.borrow_mut().push(call);
        }

        fn wait(&self) -> io::Result<()> {
            self.wait_for_exit(Instant::now() + Duration::from_secs(5))
        }
    }

    impl ExitQueue for ScriptedQueue {
        fn child(&self) -> u32 {
            Self::CHILD
        }

        fn register(&self) -> io::Result<()> {
            self.record("register");
            self.registration
                .map_or(Ok(()), |errno| Err(io::Error::from_raw_os_error(errno)))
        }

        fn peek(&self) -> io::Result<WaitReport> {
            self.record("peek");
            self.report.map_err(io::Error::from_raw_os_error)
        }

        fn block_for(&self, timeout: Duration) -> io::Result<()> {
            self.record("block");
            *self.timeout.borrow_mut() = Some(timeout);
            Ok(())
        }
    }

    #[test]
    fn a_running_child_is_registered_peeked_at_then_waited_for() {
        let queue = ScriptedQueue::running();

        queue.wait().unwrap();

        assert_eq!(queue.calls(), ["register", "peek", "block"]);
    }

    #[test]
    fn the_wait_blocks_for_the_time_left_before_the_deadline() {
        let queue = ScriptedQueue::running();

        queue.wait().unwrap();

        let timeout = queue.timeout().unwrap();
        assert!(timeout > Duration::from_secs(4) && timeout <= Duration::from_secs(5));
    }

    #[test]
    fn a_child_gone_before_registration_counts_as_exited() {
        let queue = ScriptedQueue::running().failing_registration_with(libc::ESRCH);

        queue.wait().unwrap();

        assert_eq!(queue.calls(), ["register"]);
    }

    #[test]
    fn any_other_registration_failure_is_reported() {
        let queue = ScriptedQueue::running().failing_registration_with(libc::EBADF);

        let error = queue.wait().unwrap_err();

        assert_eq!(error.raw_os_error(), Some(libc::EBADF));
        assert_eq!(queue.calls(), ["register"]);
    }

    #[test]
    fn a_child_that_exited_before_the_wait_is_not_waited_for_because_its_queue_can_stay_silent() {
        let queue = ScriptedQueue::reporting(ScriptedQueue::child_report(libc::CLD_EXITED));

        queue.wait().unwrap();

        assert_eq!(queue.calls(), ["register", "peek"]);
    }

    #[test]
    fn a_killed_child_is_not_waited_for() {
        let queue = ScriptedQueue::reporting(ScriptedQueue::child_report(libc::CLD_KILLED));

        queue.wait().unwrap();

        assert_eq!(queue.calls(), ["register", "peek"]);
    }

    #[test]
    fn a_child_that_dumped_core_is_not_waited_for() {
        let queue = ScriptedQueue::reporting(ScriptedQueue::child_report(libc::CLD_DUMPED));

        queue.wait().unwrap();

        assert_eq!(queue.calls(), ["register", "peek"]);
    }

    #[test]
    fn a_stopped_child_is_still_waited_for() {
        let queue = ScriptedQueue::reporting(ScriptedQueue::child_report(libc::CLD_STOPPED));

        queue.wait().unwrap();

        assert_eq!(queue.calls(), ["register", "peek", "block"]);
    }

    #[test]
    fn an_exit_reported_for_another_pid_is_still_waited_for() {
        let report = Ok(WaitReport::new(ScriptedQueue::OTHER, libc::CLD_EXITED));
        let queue = ScriptedQueue::reporting(report);

        queue.wait().unwrap();

        assert_eq!(queue.calls(), ["register", "peek", "block"]);
    }

    #[test]
    fn a_failed_peek_is_still_waited_for() {
        let queue = ScriptedQueue::reporting(Err(libc::ECHILD));

        queue.wait().unwrap();

        assert_eq!(queue.calls(), ["register", "peek", "block"]);
    }
}

use std::io;
use std::process::Child;

pub struct ForegroundChild {
    pid: libc::pid_t,
}

impl ForegroundChild {
    pub fn of(child: &Child) -> Option<Self> {
        libc::pid_t::try_from(child.id())
            .ok()
            .map(|pid| Self { pid })
    }

    pub fn wait(&self) -> Result<(), libc::c_int> {
        let status = self.status_after_exit();
        if status.is_some_and(Self::exited_cleanly) {
            return Ok(());
        }
        Err(status.unwrap_or_default())
    }

    fn status_after_exit(&self) -> Option<libc::c_int> {
        loop {
            let status = self.next_status()?;
            if !libc::WIFSTOPPED(status) {
                return Some(status);
            }
            self.stop_with_child();
        }
    }

    fn next_status(&self) -> Option<libc::c_int> {
        let mut status: libc::c_int = 0;
        loop {
            // SAFETY: waitpid takes the child's own pid, and status is a local the kernel fills.
            if unsafe { libc::waitpid(self.pid, &mut status, libc::WUNTRACED) } != -1 {
                return Some(status);
            }
            if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                return None;
            }
        }
    }

    fn stop_with_child(&self) {
        // SAFETY: kill takes only this process's own pid and the stopped child's pid.
        unsafe {
            libc::kill(std::process::id() as libc::pid_t, libc::SIGSTOP);
            libc::kill(self.pid, libc::SIGCONT);
        }
    }

    fn exited_cleanly(status: libc::c_int) -> bool {
        libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Child, Command};

    use super::ForegroundChild;

    struct Shell;

    impl Shell {
        fn wait_for(script: &str) -> Result<(), libc::c_int> {
            ForegroundChild::of(&Self::spawn(script)).unwrap().wait()
        }

        fn spawn(script: &str) -> Child {
            Command::new("sh").args(["-c", script]).spawn().unwrap()
        }
    }

    #[test]
    fn a_child_that_exits_zero_waits_cleanly() {
        assert_eq!(Shell::wait_for("exit 0"), Ok(()));
    }

    #[test]
    fn a_child_that_exits_non_zero_reports_its_wait_status() {
        assert!(Shell::wait_for("exit 3").is_err_and(|status| libc::WEXITSTATUS(status) == 3));
    }

    #[test]
    fn a_child_reaped_elsewhere_reports_a_failed_wait() {
        let mut child = Shell::spawn("exit 0");
        let foreground = ForegroundChild::of(&child).unwrap();
        child.wait().unwrap();
        assert_eq!(foreground.wait(), Err(0));
    }
}

use super::live_process::LiveProcess;

pub struct LaunchdDaemon;

impl LaunchdDaemon {
    const LAUNCHD: u32 = 1;

    pub fn matches(process: &LiveProcess) -> bool {
        process.session() == process.pid() && process.parent() == Self::LAUNCHD
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DaemonFixture;

    impl DaemonFixture {
        const PID: u32 = 4100;
        const RINGLEADER: u32 = 4000;
        const OWNER: u32 = 1000;

        fn process(parent: u32, session: u32) -> LiveProcess {
            LiveProcess::new(Self::PID, parent, session, Self::OWNER)
        }
    }

    #[test]
    fn a_session_leader_that_launchd_adopted_matches() {
        let server = DaemonFixture::process(LaunchdDaemon::LAUNCHD, DaemonFixture::PID);

        assert!(LaunchdDaemon::matches(&server));
    }

    #[test]
    fn a_client_leading_its_own_session_under_ringleader_does_not_match() {
        let client = DaemonFixture::process(DaemonFixture::RINGLEADER, DaemonFixture::PID);

        assert!(!LaunchdDaemon::matches(&client));
    }

    #[test]
    fn an_orphan_in_another_session_does_not_match() {
        let orphan = DaemonFixture::process(LaunchdDaemon::LAUNCHD, DaemonFixture::RINGLEADER);

        assert!(!LaunchdDaemon::matches(&orphan));
    }
}

use crate::application::{Pending, SessionService};

#[derive(Default)]
pub struct ScratchPurge {
    running: Option<Pending<()>>,
}

impl ScratchPurge {
    pub fn start(&mut self, service: &SessionService) {
        if self.running.is_none() {
            self.running = Some(service.start_scratch_purge());
        }
    }

    pub fn poll(&mut self) {
        let finished = self
            .running
            .as_ref()
            .is_some_and(|purge| purge.poll().is_some());
        if finished {
            self.running = None;
        }
    }

    #[cfg(test)]
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Deadline, ServiceFixture};

    struct PurgeFixture;

    impl PurgeFixture {
        fn poll_until_finished(purge: &mut ScratchPurge) {
            let finished = Deadline::for_background_work().wait_until(|| {
                purge.poll();
                !purge.is_running()
            });
            assert!(finished, "the purge never finished");
        }
    }

    #[test]
    fn a_started_purge_empties_the_retired_directories() {
        let fixture = ServiceFixture::new();
        let mut purge = ScratchPurge::default();

        purge.start(&fixture.service());
        PurgeFixture::poll_until_finished(&mut purge);

        assert_eq!(fixture.scratch_space().purges(), 1);
    }

    #[test]
    fn a_finished_purge_lets_the_next_change_start_another() {
        let fixture = ServiceFixture::new();
        let mut purge = ScratchPurge::default();
        purge.start(&fixture.service());
        PurgeFixture::poll_until_finished(&mut purge);

        purge.start(&fixture.service());
        PurgeFixture::poll_until_finished(&mut purge);

        assert_eq!(fixture.scratch_space().purges(), 2);
    }
}

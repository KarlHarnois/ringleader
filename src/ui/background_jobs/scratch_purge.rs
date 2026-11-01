use crate::application::{Pending, SessionService};

#[derive(Default)]
pub struct ScratchPurge {
    running: Option<Pending<()>>,
    requested_again: bool,
}

impl ScratchPurge {
    pub fn start(&mut self, service: &SessionService) {
        match self.running {
            Some(_) => self.requested_again = true,
            None => self.running = Some(service.start_scratch_purge()),
        }
    }

    pub fn poll(&mut self, service: &SessionService) {
        let finished = self
            .running
            .as_ref()
            .is_some_and(|purge| purge.poll().is_some());
        if finished {
            self.running = None;
            self.start_requested_again(service);
        }
    }

    fn start_requested_again(&mut self, service: &SessionService) {
        if std::mem::take(&mut self.requested_again) {
            self.start(service);
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
    use crate::test_support::{Deadline, FakeScratchSpace, ServiceFixture};

    struct PurgeFixture;

    impl PurgeFixture {
        fn poll_until_finished(purge: &mut ScratchPurge, service: &SessionService) {
            let finished = Deadline::for_background_work().wait_until(|| {
                purge.poll(service);
                !purge.is_running()
            });
            assert!(finished, "the purge never finished");
        }
    }

    #[test]
    fn a_started_purge_empties_the_retired_directories() {
        let fixture =
            ServiceFixture::new().with_scratch_space(FakeScratchSpace::holding_one_retired());
        let service = fixture.service();
        let mut purge = ScratchPurge::default();

        purge.start(&service);
        PurgeFixture::poll_until_finished(&mut purge, &service);

        assert_eq!(fixture.scratch_space().purged().len(), 1);
    }

    #[test]
    fn a_finished_purge_lets_the_next_change_start_another() {
        let fixture =
            ServiceFixture::new().with_scratch_space(FakeScratchSpace::holding_one_retired());
        let service = fixture.service();
        let mut purge = ScratchPurge::default();
        purge.start(&service);
        PurgeFixture::poll_until_finished(&mut purge, &service);

        purge.start(&service);
        PurgeFixture::poll_until_finished(&mut purge, &service);

        assert_eq!(fixture.scratch_space().purged().len(), 2);
    }

    #[test]
    fn a_change_during_a_purge_runs_one_more_purge_because_the_first_can_miss_its_directory() {
        let fixture =
            ServiceFixture::new().with_scratch_space(FakeScratchSpace::holding_one_retired());
        let service = fixture.service();
        let mut purge = ScratchPurge::default();

        purge.start(&service);
        purge.start(&service);
        purge.start(&service);
        PurgeFixture::poll_until_finished(&mut purge, &service);

        assert_eq!(fixture.scratch_space().purged().len(), 2);
    }
}

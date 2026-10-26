use crate::domain::{Session, SessionState, StopSkip};

pub struct RepoStopPlan {
    targets: Vec<String>,
    skips: Vec<(String, StopSkip)>,
}

impl RepoStopPlan {
    pub fn new(live_entries: &[(&Session, SessionState)]) -> Self {
        let mut plan = Self {
            targets: Vec::new(),
            skips: Vec::new(),
        };
        for (session, state) in live_entries {
            plan.place(session, *state);
        }
        plan
    }

    fn place(&mut self, session: &Session, state: SessionState) {
        let name = session.name().as_str().to_string();
        match StopSkip::of(session, state) {
            Some(skip) => self.skips.push((name, skip)),
            None => self.targets.push(name),
        }
    }

    pub fn targets(&self) -> &[String] {
        &self.targets
    }

    pub fn into_skips(self) -> Vec<(String, StopSkip)> {
        self.skips
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ServiceFixture;

    #[test]
    fn waiting_and_exited_worktree_sessions_are_stopped() {
        let waiting = ServiceFixture::session("alpha");
        let exited = ServiceFixture::session("bravo");

        let plan = RepoStopPlan::new(&[
            (&waiting, SessionState::Waiting),
            (&exited, SessionState::Exited),
        ]);

        assert_eq!(plan.targets(), ["alpha", "bravo"]);
        assert!(plan.into_skips().is_empty());
    }

    #[test]
    fn a_working_session_is_skipped_because_its_turn_would_be_lost() {
        let working = ServiceFixture::session("alpha");

        let plan = RepoStopPlan::new(&[(&working, SessionState::Working)]);

        assert!(plan.targets().is_empty());
        assert_eq!(
            plan.into_skips(),
            [("alpha".to_string(), StopSkip::State(SessionState::Working))]
        );
    }

    #[test]
    fn a_main_checkout_session_is_skipped_because_its_restart_starts_fresh() {
        let main_checkout = ServiceFixture::main_checkout_session("alpha");

        let plan = RepoStopPlan::new(&[(&main_checkout, SessionState::Waiting)]);

        assert!(plan.targets().is_empty());
        assert_eq!(
            plan.into_skips(),
            [("alpha".to_string(), StopSkip::MainCheckout)]
        );
    }
}

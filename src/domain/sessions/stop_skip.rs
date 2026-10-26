use super::session::Session;
use super::session_state::SessionState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopSkip {
    State(SessionState),
    MainCheckout,
}

impl StopSkip {
    const MAIN_CHECKOUT: &'static str = "in the main checkout";

    pub fn of(session: &Session, state: SessionState) -> Option<Self> {
        if session.runs_in_main_checkout() {
            return Some(Self::MainCheckout);
        }
        (!state.allows_stop()).then_some(Self::State(state))
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::State(state) => state.label(),
            Self::MainCheckout => Self::MAIN_CHECKOUT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ServiceFixture;

    #[test]
    fn a_waiting_worktree_session_may_stop() {
        let session = ServiceFixture::session("fix-auth");

        assert_eq!(StopSkip::of(&session, SessionState::Waiting), None);
    }

    #[test]
    fn a_working_worktree_session_is_skipped_for_its_state() {
        let session = ServiceFixture::session("fix-auth");

        assert_eq!(
            StopSkip::of(&session, SessionState::Working),
            Some(StopSkip::State(SessionState::Working))
        );
    }

    #[test]
    fn a_working_main_checkout_session_is_skipped_for_the_main_checkout_because_waiting_never_helps()
     {
        let session = ServiceFixture::main_checkout_session("fix-auth");

        assert_eq!(
            StopSkip::of(&session, SessionState::Working),
            Some(StopSkip::MainCheckout)
        );
    }
}

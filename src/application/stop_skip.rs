use crate::domain::{Session, SessionState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopSkip {
    State(SessionState),
    MainCheckout,
}

impl StopSkip {
    const MAIN_CHECKOUT: &'static str = "in the main checkout";

    pub fn of(session: &Session, state: SessionState) -> Option<Self> {
        if !state.allows_stop() {
            return Some(Self::State(state));
        }
        session
            .runs_in_main_checkout()
            .then_some(Self::MainCheckout)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::State(state) => state.label(),
            Self::MainCheckout => Self::MAIN_CHECKOUT,
        }
    }
}

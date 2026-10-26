#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SessionState {
    Working,
    Waiting,
    Exited,
    Stopped,
    Unmanaged,
}

impl SessionState {
    pub const ALL: [SessionState; 5] = [
        SessionState::Working,
        SessionState::Waiting,
        SessionState::Exited,
        SessionState::Stopped,
        SessionState::Unmanaged,
    ];

    pub fn is_bell_edge(previous: SessionState, current: SessionState) -> bool {
        previous == SessionState::Working && current == SessionState::Waiting
    }

    pub fn runs_agent(self) -> bool {
        matches!(self, SessionState::Working | SessionState::Waiting)
    }

    pub fn allows_restart(self) -> bool {
        matches!(self, SessionState::Exited | SessionState::Stopped)
    }

    pub fn allows_stop(self) -> bool {
        matches!(self, SessionState::Waiting | SessionState::Exited)
    }

    pub fn label(self) -> &'static str {
        match self {
            SessionState::Working => "working",
            SessionState::Waiting => "waiting",
            SessionState::Exited => "exited",
            SessionState::Stopped => "stopped",
            SessionState::Unmanaged => "unmanaged",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SessionState::*;
    use super::*;

    #[test]
    fn only_working_and_waiting_run_the_agent() {
        let running: Vec<SessionState> = SessionState::ALL
            .into_iter()
            .filter(|state| state.runs_agent())
            .collect();

        assert_eq!(running, [Working, Waiting]);
    }

    #[test]
    fn only_exited_and_stopped_allow_restart() {
        let restartable: Vec<SessionState> = SessionState::ALL
            .into_iter()
            .filter(|state| state.allows_restart())
            .collect();

        assert_eq!(restartable, [Exited, Stopped]);
    }

    #[test]
    fn only_waiting_and_exited_allow_stop_because_stopping_a_working_agent_drops_its_turn() {
        let stoppable: Vec<SessionState> = SessionState::ALL
            .into_iter()
            .filter(|state| state.allows_stop())
            .collect();

        assert_eq!(stoppable, [Waiting, Exited]);
    }

    #[test]
    fn working_followed_by_waiting_is_the_bell_edge() {
        assert!(SessionState::is_bell_edge(Working, Waiting));
    }

    #[test]
    fn no_other_transition_is_the_bell_edge() {
        for previous in SessionState::ALL {
            for current in SessionState::ALL {
                let is_working_to_waiting = previous == Working && current == Waiting;
                if !is_working_to_waiting {
                    assert!(!SessionState::is_bell_edge(previous, current));
                }
            }
        }
    }

    #[test]
    fn labels_are_stable_lowercase_strings() {
        let labels: Vec<&str> = SessionState::ALL
            .iter()
            .map(|state| state.label())
            .collect();

        assert_eq!(
            labels,
            ["working", "waiting", "exited", "stopped", "unmanaged"]
        );
    }

    #[test]
    fn states_sort_by_display_priority() {
        let mut states = vec![Unmanaged, Stopped, Waiting, Exited, Working];

        states.sort();

        assert_eq!(states, SessionState::ALL);
    }
}

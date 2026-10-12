#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SessionState {
    Working,
    Waiting,
    Exited,
    Stopped,
    Unmanaged,
}

impl SessionState {
    pub fn is_bell_edge(previous: SessionState, current: SessionState) -> bool {
        previous == SessionState::Working && current == SessionState::Waiting
    }

    pub fn runs_agent(self) -> bool {
        matches!(self, SessionState::Working | SessionState::Waiting)
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

    const ALL_STATES: [SessionState; 5] = [Working, Waiting, Exited, Stopped, Unmanaged];

    #[test]
    fn only_working_and_waiting_run_the_agent() {
        let running: Vec<SessionState> = ALL_STATES
            .into_iter()
            .filter(|state| state.runs_agent())
            .collect();

        assert_eq!(running, [Working, Waiting]);
    }

    #[test]
    fn working_followed_by_waiting_is_the_bell_edge() {
        assert!(SessionState::is_bell_edge(Working, Waiting));
    }

    #[test]
    fn no_other_transition_is_the_bell_edge() {
        for previous in ALL_STATES {
            for current in ALL_STATES {
                let is_working_to_waiting = previous == Working && current == Waiting;
                if !is_working_to_waiting {
                    assert!(!SessionState::is_bell_edge(previous, current));
                }
            }
        }
    }

    #[test]
    fn labels_are_stable_lowercase_strings() {
        let labels: Vec<&str> = ALL_STATES.iter().map(|state| state.label()).collect();

        assert_eq!(
            labels,
            ["working", "waiting", "exited", "stopped", "unmanaged"]
        );
    }

    #[test]
    fn states_sort_by_display_priority() {
        let mut states = vec![Unmanaged, Stopped, Waiting, Exited, Working];

        states.sort();

        assert_eq!(states, ALL_STATES);
    }
}

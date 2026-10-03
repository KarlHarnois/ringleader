use super::session_observation::SessionObservation;
use super::session_state::SessionState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionClassifier {
    activity_idle_seconds: u64,
}

impl SessionClassifier {
    pub fn new(activity_idle_seconds: u64) -> Self {
        Self {
            activity_idle_seconds,
        }
    }

    pub fn classify(&self, observation: Option<&SessionObservation>, now: u64) -> SessionState {
        let Some(observation) = observation else {
            return SessionState::Stopped;
        };
        if observation.metadata().is_none() {
            SessionState::Unmanaged
        } else if observation.pane_dead() {
            SessionState::Exited
        } else if self.is_recently_active(observation, now) {
            SessionState::Working
        } else {
            SessionState::Waiting
        }
    }

    fn is_recently_active(&self, observation: &SessionObservation, now: u64) -> bool {
        let activity_age = now.saturating_sub(observation.activity_at());
        activity_age < self.activity_idle_seconds
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::domain::repo_name::RepoName;
    use crate::domain::session::TmuxAddress;
    use crate::domain::session_metadata::{SessionMetadata, SessionRepo, SessionWorktree};
    use crate::domain::session_observation::AgentPane;

    const NOW: u64 = 1_700_000_100;
    const IDLE_SECONDS: u64 = 3;

    struct ClassifierFixture;

    impl ClassifierFixture {
        fn classify(observation: &SessionObservation) -> SessionState {
            SessionClassifier::new(IDLE_SECONDS).classify(Some(observation), NOW)
        }

        fn managed(agent_pane: AgentPane) -> SessionObservation {
            Self::unmanaged(agent_pane).with_metadata(Self::metadata())
        }

        fn unmanaged(agent_pane: AgentPane) -> SessionObservation {
            let tmux = TmuxAddress::new("ringleader-fix-auth", Path::new("/tmp/tmux-1000/default"));
            SessionObservation::new(tmux, NOW - 600).with_agent_pane(agent_pane)
        }

        fn metadata() -> SessionMetadata {
            SessionMetadata::new(
                SessionRepo::new(
                    RepoName::parse("example-project").unwrap(),
                    Path::new("/repos/example-project"),
                ),
                SessionWorktree::new(
                    Path::new("/worktrees/example-project/fix-auth"),
                    "ringleader/fix-auth",
                    "abc123",
                ),
                "claude",
            )
        }
    }

    #[test]
    fn a_missing_session_is_stopped() {
        let state = SessionClassifier::new(IDLE_SECONDS).classify(None, NOW);

        assert_eq!(state, SessionState::Stopped);
    }

    #[test]
    fn a_session_without_valid_metadata_is_unmanaged() {
        let observation = ClassifierFixture::unmanaged(AgentPane::live(NOW));

        assert_eq!(
            ClassifierFixture::classify(&observation),
            SessionState::Unmanaged
        );
    }

    #[test]
    fn a_dead_agent_pane_is_exited() {
        let observation = ClassifierFixture::managed(AgentPane::dead(NOW));

        assert_eq!(
            ClassifierFixture::classify(&observation),
            SessionState::Exited
        );
    }

    #[test]
    fn missing_metadata_wins_over_a_dead_pane() {
        let observation = ClassifierFixture::unmanaged(AgentPane::dead(NOW));

        assert_eq!(
            ClassifierFixture::classify(&observation),
            SessionState::Unmanaged
        );
    }

    #[test]
    fn fresh_activity_is_working() {
        let observation = ClassifierFixture::managed(AgentPane::live(NOW));

        assert_eq!(
            ClassifierFixture::classify(&observation),
            SessionState::Working
        );
    }

    #[test]
    fn activity_one_second_younger_than_the_threshold_is_working() {
        let observation = ClassifierFixture::managed(AgentPane::live(NOW - IDLE_SECONDS + 1));

        assert_eq!(
            ClassifierFixture::classify(&observation),
            SessionState::Working
        );
    }

    #[test]
    fn activity_exactly_at_the_threshold_is_waiting() {
        let observation = ClassifierFixture::managed(AgentPane::live(NOW - IDLE_SECONDS));

        assert_eq!(
            ClassifierFixture::classify(&observation),
            SessionState::Waiting
        );
    }

    #[test]
    fn stale_activity_is_waiting() {
        let observation = ClassifierFixture::managed(AgentPane::live(NOW - 600));

        assert_eq!(
            ClassifierFixture::classify(&observation),
            SessionState::Waiting
        );
    }

    #[test]
    fn an_age_of_one_with_a_threshold_of_two_is_working() {
        let observation = ClassifierFixture::managed(AgentPane::live(NOW - 1));

        let state = SessionClassifier::new(2).classify(Some(&observation), NOW);

        assert_eq!(state, SessionState::Working);
    }

    #[test]
    fn activity_after_now_counts_as_fresh() {
        let observation = ClassifierFixture::managed(AgentPane::live(NOW + 1));

        assert_eq!(
            ClassifierFixture::classify(&observation),
            SessionState::Working
        );
    }
}

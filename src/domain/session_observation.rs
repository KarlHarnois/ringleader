use std::path::Path;

use super::session::TmuxAddress;
use super::session_metadata::SessionMetadata;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionObservation {
    tmux: TmuxAddress,
    created_at: u64,
    agent_pane: Option<AgentPane>,
    metadata: Option<SessionMetadata>,
}

impl SessionObservation {
    pub fn new(tmux: TmuxAddress, created_at: u64) -> Self {
        Self {
            tmux,
            created_at,
            agent_pane: None,
            metadata: None,
        }
    }

    pub fn with_agent_pane(self, agent_pane: AgentPane) -> Self {
        Self {
            agent_pane: Some(agent_pane),
            ..self
        }
    }

    pub fn with_metadata(self, metadata: SessionMetadata) -> Self {
        Self {
            metadata: Some(metadata),
            ..self
        }
    }

    pub fn without_metadata(&self) -> Self {
        Self {
            metadata: None,
            ..self.clone()
        }
    }

    pub fn tmux(&self) -> &TmuxAddress {
        &self.tmux
    }

    pub fn tmux_name(&self) -> &str {
        self.tmux.session_name()
    }

    pub fn socket_path(&self) -> &Path {
        self.tmux.socket_path()
    }

    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    pub fn metadata(&self) -> Option<&SessionMetadata> {
        self.metadata.as_ref()
    }

    pub fn pane_dead(&self) -> bool {
        self.agent_pane.as_ref().is_none_or(|pane| pane.dead)
    }

    pub fn attached(&self) -> bool {
        self.agent_pane.as_ref().is_some_and(|pane| pane.attached)
    }

    pub fn activity_at(&self) -> u64 {
        self.agent_pane.as_ref().map_or(0, |pane| pane.activity_at)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentPane {
    dead: bool,
    activity_at: u64,
    attached: bool,
}

impl AgentPane {
    pub fn live(activity_at: u64) -> Self {
        Self {
            dead: false,
            activity_at,
            attached: false,
        }
    }

    pub fn dead(activity_at: u64) -> Self {
        Self {
            dead: true,
            ..Self::live(activity_at)
        }
    }

    pub fn shown_by_a_client(self) -> Self {
        Self {
            attached: true,
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ObservationFixture;

    impl ObservationFixture {
        fn bare() -> SessionObservation {
            let tmux = TmuxAddress::new("ringleader-fix-auth", Path::new("/tmp/tmux-1000/default"));
            SessionObservation::new(tmux, 1_700_000_000)
        }
    }

    #[test]
    fn a_session_without_a_tagged_agent_pane_reads_as_dead() {
        assert!(ObservationFixture::bare().pane_dead());
    }

    #[test]
    fn a_session_without_a_tagged_agent_pane_is_not_attached() {
        assert!(!ObservationFixture::bare().attached());
    }

    #[test]
    fn a_client_on_the_agent_window_marks_it_attached() {
        let observation =
            ObservationFixture::bare().with_agent_pane(AgentPane::live(10).shown_by_a_client());

        assert!(observation.attached());
    }
}

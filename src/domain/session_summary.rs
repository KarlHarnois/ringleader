use super::diff_stat::DiffStat;
use super::session::Session;
use super::session_metadata::SessionMetadata;
use super::session_observation::SessionObservation;
use super::session_state::SessionState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    subject: SummarySubject,
    state: SessionState,
    attached: bool,
    diff: Option<DiffStat>,
}

impl SessionSummary {
    pub fn managed(session: Session, state: SessionState, attached: bool) -> Self {
        Self {
            subject: SummarySubject::Managed(Box::new(session)),
            state,
            attached,
            diff: None,
        }
    }

    pub fn unmanaged(session: UnmanagedSession) -> Self {
        let attached = session.observation.attached();
        Self {
            subject: SummarySubject::Unmanaged(Box::new(session)),
            state: SessionState::Unmanaged,
            attached,
            diff: None,
        }
    }

    pub fn with_diff(self, diff: DiffStat) -> Self {
        let diff = match self.subject {
            SummarySubject::Managed(_) => Some(diff),
            SummarySubject::Unmanaged(_) => None,
        };
        Self { diff, ..self }
    }

    pub fn name(&self) -> &str {
        match &self.subject {
            SummarySubject::Managed(session) => session.name().as_str(),
            SummarySubject::Unmanaged(session) => &session.label,
        }
    }

    pub fn tmux_name(&self) -> &str {
        match &self.subject {
            SummarySubject::Managed(session) => session.tmux_name(),
            SummarySubject::Unmanaged(session) => session.observation.tmux_name(),
        }
    }

    pub fn metadata(&self) -> Option<&SessionMetadata> {
        match &self.subject {
            SummarySubject::Managed(session) => Some(session.metadata()),
            SummarySubject::Unmanaged(session) => session.observation.metadata(),
        }
    }

    pub fn session(&self) -> Option<&Session> {
        match &self.subject {
            SummarySubject::Managed(session) => Some(session),
            SummarySubject::Unmanaged(_) => None,
        }
    }

    pub fn unmanaged_session(&self) -> Option<&UnmanagedSession> {
        match &self.subject {
            SummarySubject::Managed(_) => None,
            SummarySubject::Unmanaged(session) => Some(session),
        }
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn attached(&self) -> bool {
        self.attached
    }

    pub fn diff(&self) -> Option<DiffStat> {
        self.diff
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SummarySubject {
    Managed(Box<Session>),
    Unmanaged(Box<UnmanagedSession>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmanagedSession {
    label: String,
    observation: SessionObservation,
    shadowed_entry: Option<Session>,
}

impl UnmanagedSession {
    pub fn new(label: &str, observation: SessionObservation) -> Self {
        Self {
            label: label.to_string(),
            observation,
            shadowed_entry: None,
        }
    }

    pub fn shadowing(entry: Session, observation: SessionObservation) -> Self {
        Self {
            label: entry.name().to_string(),
            observation,
            shadowed_entry: Some(entry),
        }
    }

    pub fn observation(&self) -> &SessionObservation {
        &self.observation
    }

    pub fn shadowed_entry(&self) -> Option<&Session> {
        self.shadowed_entry.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::domain::session::TmuxAddress;

    #[test]
    fn an_unmanaged_summary_never_carries_a_diff() {
        let tmux = TmuxAddress::new("ringleader-mystery", Path::new("/tmp/tmux-1000/default"));
        let observation = SessionObservation::new(tmux, 1_700_000_000);
        let summary = SessionSummary::unmanaged(UnmanagedSession::new("mystery", observation));

        let summary = summary.with_diff(DiffStat::default());

        assert_eq!(summary.diff(), None);
    }

    #[test]
    fn an_unmanaged_summary_is_in_the_unmanaged_state() {
        let tmux = TmuxAddress::new("ringleader-mystery", Path::new("/tmp/tmux-1000/default"));
        let observation = SessionObservation::new(tmux, 1_700_000_000);

        let summary = SessionSummary::unmanaged(UnmanagedSession::new("mystery", observation));

        assert_eq!(summary.state(), SessionState::Unmanaged);
    }
}

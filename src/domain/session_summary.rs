use super::diff_stat::DiffStat;
use super::pull_request::PullRequest;
use super::session::Session;
use super::session_metadata::SessionMetadata;
use super::session_state::SessionState;
use super::unmanaged_session::UnmanagedSession;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    subject: SummarySubject,
    state: SessionState,
    attached: bool,
    diff: Option<DiffStat>,
    pull_request: Option<PullRequest>,
}

impl SessionSummary {
    pub fn managed(session: Session, state: SessionState, attached: bool) -> Self {
        Self {
            subject: SummarySubject::Managed(Box::new(session)),
            state,
            attached,
            diff: None,
            pull_request: None,
        }
    }

    pub fn unmanaged(session: UnmanagedSession) -> Self {
        let attached = session.observation().attached();
        Self {
            subject: SummarySubject::Unmanaged(Box::new(session)),
            state: SessionState::Unmanaged,
            attached,
            diff: None,
            pull_request: None,
        }
    }

    pub fn with_diff(self, diff: DiffStat) -> Self {
        let diff = match self.subject {
            SummarySubject::Managed(_) => Some(diff),
            SummarySubject::Unmanaged(_) => None,
        };
        Self { diff, ..self }
    }

    pub fn with_pull_request(self, pull_request: PullRequest) -> Self {
        let pull_request = match self.subject {
            SummarySubject::Managed(_) => Some(pull_request),
            SummarySubject::Unmanaged(_) => None,
        };
        Self {
            pull_request,
            ..self
        }
    }

    pub fn name(&self) -> &str {
        match &self.subject {
            SummarySubject::Managed(session) => session.name().as_str(),
            SummarySubject::Unmanaged(session) => session.label(),
        }
    }

    pub fn tmux_name(&self) -> &str {
        match &self.subject {
            SummarySubject::Managed(session) => session.tmux_name(),
            SummarySubject::Unmanaged(session) => session.observation().tmux_name(),
        }
    }

    pub fn metadata(&self) -> Option<&SessionMetadata> {
        match &self.subject {
            SummarySubject::Managed(session) => Some(session.metadata()),
            SummarySubject::Unmanaged(session) => session.observation().metadata(),
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

    pub fn pull_request(&self) -> Option<PullRequest> {
        self.pull_request
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SummarySubject {
    Managed(Box<Session>),
    Unmanaged(Box<UnmanagedSession>),
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::domain::session::TmuxAddress;
    use crate::domain::session_observation::SessionObservation;

    #[test]
    fn an_unmanaged_summary_never_carries_a_diff() {
        let tmux = TmuxAddress::new("ringleader-mystery", Path::new("/tmp/tmux-1000/default"));
        let observation = SessionObservation::new(tmux, 1_700_000_000);
        let summary = SessionSummary::unmanaged(UnmanagedSession::new("mystery", observation));

        let summary = summary.with_diff(DiffStat::default());

        assert_eq!(summary.diff(), None);
    }

    #[test]
    fn an_unmanaged_summary_never_carries_a_pull_request() {
        let tmux = TmuxAddress::new("ringleader-mystery", Path::new("/tmp/tmux-1000/default"));
        let observation = SessionObservation::new(tmux, 1_700_000_000);
        let summary = SessionSummary::unmanaged(UnmanagedSession::new("mystery", observation));

        let summary = summary.with_pull_request(crate::test_support::ServiceFixture::pull_request(
            1,
            crate::domain::PullRequestState::Open,
        ));

        assert_eq!(summary.pull_request(), None);
    }

    #[test]
    fn an_unmanaged_summary_is_in_the_unmanaged_state() {
        let tmux = TmuxAddress::new("ringleader-mystery", Path::new("/tmp/tmux-1000/default"));
        let observation = SessionObservation::new(tmux, 1_700_000_000);

        let summary = SessionSummary::unmanaged(UnmanagedSession::new("mystery", observation));

        assert_eq!(summary.state(), SessionState::Unmanaged);
    }
}

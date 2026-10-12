use std::path::PathBuf;

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
            SummarySubject::Unmanaged(session) => session.label(),
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
    shadowed: Option<(Session, PairMismatch)>,
}

impl UnmanagedSession {
    pub fn new(label: &str, observation: SessionObservation) -> Self {
        Self {
            label: label.to_string(),
            observation,
            shadowed: None,
        }
    }

    pub fn shadowing(
        entry: Session,
        observation: SessionObservation,
        mismatch: PairMismatch,
    ) -> Self {
        Self {
            label: entry.name().to_string(),
            observation,
            shadowed: Some((entry, mismatch)),
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn advice(&self) -> Option<String> {
        let (_, mismatch) = self.shadowed.as_ref()?;
        let tmux_name = self.observation.tmux_name();
        let socket = self.observation.socket_path().display();
        Some(format!(
            "tmux session {tmux_name} on {socket} {}. Inspect it, then end it with tmux -S {socket} kill-session -t '={tmux_name}:' to restart or delete this session.",
            mismatch.describe()
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairMismatch {
    NoValidMetadata,
    OtherWorktree(PathBuf),
    OtherSocket(PathBuf),
}

impl PairMismatch {
    fn describe(&self) -> String {
        match self {
            PairMismatch::NoValidMetadata => "has no valid Ringleader metadata".to_string(),
            PairMismatch::OtherWorktree(path) => {
                format!("names another worktree, {}", path.display())
            }
            PairMismatch::OtherSocket(path) => {
                format!(
                    "runs on another socket than the recorded {}",
                    path.display()
                )
            }
        }
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
    fn a_shadowing_session_advises_how_to_end_it() {
        let tmux = TmuxAddress::new("ringleader-fix-auth", Path::new("/tmp/s"));
        let observation = SessionObservation::new(tmux, 1_700_000_000);
        let entry = crate::test_support::ServiceFixture::session("fix-auth");

        let unmanaged =
            UnmanagedSession::shadowing(entry, observation, PairMismatch::NoValidMetadata);

        assert_eq!(
            unmanaged.advice().unwrap(),
            "tmux session ringleader-fix-auth on /tmp/s has no valid Ringleader metadata. Inspect it, then end it with tmux -S /tmp/s kill-session -t '=ringleader-fix-auth:' to restart or delete this session."
        );
    }

    #[test]
    fn an_unmanaged_summary_is_in_the_unmanaged_state() {
        let tmux = TmuxAddress::new("ringleader-mystery", Path::new("/tmp/tmux-1000/default"));
        let observation = SessionObservation::new(tmux, 1_700_000_000);

        let summary = SessionSummary::unmanaged(UnmanagedSession::new("mystery", observation));

        assert_eq!(summary.state(), SessionState::Unmanaged);
    }
}

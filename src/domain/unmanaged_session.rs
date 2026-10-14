use std::path::PathBuf;

use super::session::Session;
use super::session_observation::SessionObservation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmanagedSession {
    label: String,
    observation: SessionObservation,
    missing_prefix: Option<String>,
    shadowed: Option<(Session, PairMismatch)>,
}

impl UnmanagedSession {
    const EXACT_LABEL: &'static str = "=";

    pub fn new(label: &str, observation: SessionObservation) -> Self {
        Self {
            label: label.to_string(),
            observation,
            missing_prefix: None,
            shadowed: None,
        }
    }

    pub fn unprefixed(observation: SessionObservation) -> Self {
        Self {
            label: format!("{}{}", Self::EXACT_LABEL, observation.tmux_name()),
            observation,
            missing_prefix: None,
            shadowed: None,
        }
    }

    pub fn missing_only(self, prefix: &str) -> Self {
        Self {
            missing_prefix: Some(prefix.to_string()),
            ..self
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
            missing_prefix: None,
            shadowed: Some((entry, mismatch)),
        }
    }

    pub fn note(&self) -> String {
        self.advice().unwrap_or_else(|| {
            format!(
                "{} {}, so it can only be attached.",
                self.observation.tmux_name(),
                self.attach_only_reason()
            )
        })
    }

    fn attach_only_reason(&self) -> String {
        match &self.missing_prefix {
            Some(prefix) => format!("lacks the {prefix} prefix of Ringleader's sessions"),
            None => PairMismatch::NoValidMetadata.describe(),
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn observation(&self) -> &SessionObservation {
        &self.observation
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
}

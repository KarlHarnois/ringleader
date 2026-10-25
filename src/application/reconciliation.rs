use super::service_error::ServiceError;
use crate::domain::{
    RepoGroup, Session, SessionMetadata, SessionName, SessionObservation, SessionState,
    SessionSummary, UnmanagedSession,
};

pub struct Reconciliation {
    entries: Vec<Session>,
    rows: Vec<SessionSummary>,
    observations: Vec<SessionObservation>,
    changed: bool,
}

impl Reconciliation {
    pub fn new(entries: &[Session]) -> Self {
        Self {
            entries: entries.to_vec(),
            rows: Vec::new(),
            observations: Vec::new(),
            changed: false,
        }
    }

    pub fn add_row(&mut self, row: SessionSummary) {
        self.rows.push(row);
    }

    pub fn adopt(&mut self, session: Session, row: SessionSummary) {
        self.entries.push(session);
        self.rows.push(row);
        self.changed = true;
    }

    pub fn rekey(&mut self, index: usize, session: Session) {
        self.entries[index] = session;
        self.changed = true;
    }

    pub fn record_observations(&mut self, observations: Vec<SessionObservation>) {
        self.observations = observations;
    }

    pub fn entries(&self) -> &[Session] {
        &self.entries
    }

    pub fn changed(&self) -> bool {
        self.changed
    }

    pub fn rows(&self) -> &[SessionSummary] {
        &self.rows
    }

    pub fn managed_entries(&self) -> Vec<(&Session, SessionState)> {
        self.entries
            .iter()
            .map(|entry| {
                let state = self
                    .rows
                    .iter()
                    .find(|row| row.name() == entry.name().as_str())
                    .map_or(SessionState::Stopped, SessionSummary::state);
                (entry, state)
            })
            .collect()
    }

    pub fn restartable_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .managed_entries()
            .into_iter()
            .filter(|(_, state)| state.allows_restart())
            .map(|(entry, _)| entry.name().as_str().to_string())
            .collect();
        names.sort();
        names
    }

    pub fn live_entries_in(&self, repo: &RepoGroup) -> Vec<(&Session, SessionState)> {
        let mut entries: Vec<(&Session, SessionState)> = self
            .managed_entries()
            .into_iter()
            .filter(|(entry, state)| {
                *state != SessionState::Stopped && repo.path() == Some(entry.repo_path())
            })
            .collect();
        entries.sort_by(|(left, _), (right, _)| left.name().as_str().cmp(right.name().as_str()));
        entries
    }

    pub fn into_summaries(self) -> Vec<SessionSummary> {
        self.rows
    }

    pub fn row(&self, name: &str) -> Result<&SessionSummary, ServiceError> {
        self.rows
            .iter()
            .find(|row| row.name() == name)
            .ok_or_else(|| ServiceError::NotFound(format!("no session named {name}")))
    }

    pub fn managed(&self, name: &str) -> Result<ManagedRow<'_>, ServiceError> {
        let row = self.row(name)?;
        let session = row.session().ok_or_else(|| Self::unmanaged_refusal(row))?;
        Ok(ManagedRow {
            session,
            state: row.state(),
            observation: self.observation(session.tmux_name()),
        })
    }

    pub fn is_live_tmux_name(&self, tmux_name: &str) -> bool {
        self.observation(tmux_name).is_some()
    }

    pub fn refuse_taken_name(
        &self,
        name: &SessionName,
        tmux_name: &str,
    ) -> Result<(), ServiceError> {
        let taken_by_entry = self.entries.iter().any(|entry| entry.name() == name);
        if taken_by_entry || self.is_live_tmux_name(tmux_name) {
            return Err(ServiceError::Conflict(format!(
                "a session named {name} already exists"
            )));
        }
        Ok(())
    }

    pub fn claimant_of(&self, session: &Session) -> Option<&SessionObservation> {
        self.observations.iter().find(|observation| {
            observation.tmux_name() != session.tmux_name()
                && observation
                    .metadata()
                    .is_some_and(|metadata| Self::claims_worktree(metadata, session))
        })
    }

    pub fn entries_with(&self, old_name: &str, replacement: Session) -> Vec<Session> {
        self.entries
            .iter()
            .map(|entry| {
                if entry.name().as_str() == old_name {
                    replacement.clone()
                } else {
                    entry.clone()
                }
            })
            .collect()
    }

    pub fn entries_without(&self, name: &str) -> Vec<Session> {
        self.entries
            .iter()
            .filter(|entry| entry.name().as_str() != name)
            .cloned()
            .collect()
    }

    fn observation(&self, tmux_name: &str) -> Option<&SessionObservation> {
        self.observations
            .iter()
            .find(|observation| observation.tmux_name() == tmux_name)
    }

    fn unmanaged_refusal(row: &SessionSummary) -> ServiceError {
        let advice = row.unmanaged_session().and_then(UnmanagedSession::advice);
        ServiceError::Conflict(advice.unwrap_or_else(|| {
            format!(
                "session {} is not managed by Ringleader, it can only be attached",
                row.name()
            )
        }))
    }
}

pub struct ManagedRow<'a> {
    session: &'a Session,
    state: SessionState,
    observation: Option<&'a SessionObservation>,
}

impl ManagedRow<'_> {
    pub fn session(&self) -> &Session {
        self.session
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn is_live(&self) -> bool {
        self.observation.is_some()
    }
}

impl Reconciliation {
    fn claims_worktree(metadata: &SessionMetadata, entry: &Session) -> bool {
        metadata.has_absolute_paths() && metadata.worktree_path() == entry.worktree_path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ServiceFixture;

    #[test]
    fn a_name_a_registry_entry_uses_is_taken_even_without_a_live_tmux_session() {
        let entry = ServiceFixture::session("fix-auth");
        let reconciliation = Reconciliation::new(std::slice::from_ref(&entry));

        let refusal = reconciliation.refuse_taken_name(entry.name(), "ringleader-not-running");

        assert_eq!(
            refusal,
            Err(ServiceError::Conflict(
                "a session named fix-auth already exists".to_string()
            ))
        );
    }
}

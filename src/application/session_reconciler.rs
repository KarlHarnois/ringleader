use super::ports::HostProbe;
use super::reconciliation::Reconciliation;
use super::worktree_containment::WorktreeContainment;
use crate::domain::{
    Config, NamingScheme, PairMismatch, Registration, Session, SessionClassifier, SessionMetadata,
    SessionName, SessionObservation, SessionState, SessionSummary, UnmanagedSession,
};

pub struct SessionReconciler<'a> {
    config: &'a Config,
    classifier: SessionClassifier,
    containment: WorktreeContainment<'a>,
    now: u64,
}

impl<'a> SessionReconciler<'a> {
    pub fn new(config: &'a Config, host: &'a dyn HostProbe, now: u64) -> Self {
        Self {
            config,
            classifier: SessionClassifier::new(config.activity_idle_seconds()),
            containment: WorktreeContainment::new(host),
            now,
        }
    }

    pub fn reconcile(
        &self,
        entries: &[Session],
        observations: Vec<SessionObservation>,
    ) -> Reconciliation {
        let mut reconciliation = Reconciliation::new(entries);
        for entry in entries {
            let namesake = observations
                .iter()
                .find(|observation| observation.tmux_name() == entry.tmux_name());
            reconciliation.add_row(self.entry_row(entry, namesake));
        }
        for observation in &observations {
            if !reconciliation.is_registered_tmux_name(observation.tmux_name()) {
                self.place_unregistered(&mut reconciliation, observation);
            }
        }
        reconciliation.record_observations(observations);
        reconciliation
    }

    fn entry_row(&self, entry: &Session, namesake: Option<&SessionObservation>) -> SessionSummary {
        let Some(observation) = namesake else {
            return SessionSummary::managed(entry.clone(), SessionState::Stopped, false);
        };
        match self.pair_mismatch(entry, observation) {
            None => self.managed_row(entry.clone(), observation),
            Some(mismatch) => SessionSummary::unmanaged(UnmanagedSession::shadowing(
                entry.clone(),
                observation.clone(),
                mismatch,
            )),
        }
    }

    fn place_unregistered(
        &self,
        reconciliation: &mut Reconciliation,
        observation: &SessionObservation,
    ) {
        let Some(stripped) = self
            .config
            .naming()
            .strip_tmux_prefix(observation.tmux_name())
        else {
            return;
        };
        match self.adoption(reconciliation, stripped, observation) {
            Some(adopted) => {
                let row = self.managed_row(adopted.clone(), observation);
                reconciliation.adopt(adopted, row);
            }
            None => reconciliation.add_row(SessionSummary::unmanaged(UnmanagedSession::new(
                stripped,
                observation.clone(),
            ))),
        }
    }

    fn managed_row(&self, session: Session, observation: &SessionObservation) -> SessionSummary {
        let state = self.classifier.classify(Some(observation), self.now);
        SessionSummary::managed(session, state, observation.attached())
    }

    fn pair_mismatch(
        &self,
        entry: &Session,
        observation: &SessionObservation,
    ) -> Option<PairMismatch> {
        let Some(metadata) = self.valid_metadata(observation, entry.naming()) else {
            return Some(PairMismatch::NoValidMetadata);
        };
        if metadata.worktree_path() != entry.worktree_path() {
            return Some(PairMismatch::OtherWorktree(
                metadata.worktree_path().to_path_buf(),
            ));
        }
        if observation.socket_path() != entry.socket_path() {
            return Some(PairMismatch::OtherSocket(entry.socket_path().to_path_buf()));
        }
        None
    }

    fn adoption(
        &self,
        reconciliation: &Reconciliation,
        stripped: &str,
        observation: &SessionObservation,
    ) -> Option<Session> {
        let name = SessionName::from_slug(stripped).ok()?;
        let metadata = self.valid_metadata(observation, self.config.naming())?;
        if reconciliation.conflicts_with_adoption(&name, metadata) {
            return None;
        }
        let registration = Registration::new(
            observation.tmux().clone(),
            self.config.naming().clone(),
            observation.created_at(),
        );
        Some(Session::new(name, metadata.clone(), registration))
    }

    fn valid_metadata<'o>(
        &self,
        observation: &'o SessionObservation,
        naming: &NamingScheme,
    ) -> Option<&'o SessionMetadata> {
        observation.metadata().filter(|metadata| {
            metadata.has_absolute_paths()
                && naming.owns_branch(metadata.branch())
                && self
                    .containment
                    .contains(naming.worktree_root(), metadata.worktree_path())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::AgentPane;
    use crate::test_support::{FakeMultiplexer, FakeSessionRegistry, ServiceFixture};

    struct ObserveFixture;

    impl ObserveFixture {
        fn observe(fixture: &ServiceFixture) -> Vec<SessionSummary> {
            fixture.service().observe(true).unwrap()
        }

        fn only_row(fixture: &ServiceFixture) -> SessionSummary {
            let mut rows = Self::observe(fixture);
            assert_eq!(rows.len(), 1);
            rows.pop().unwrap()
        }
    }

    #[test]
    fn a_registry_entry_without_a_live_session_is_stopped() {
        let fixture = ServiceFixture::new().with_sessions(&[ServiceFixture::session("fix-auth")]);

        let row = ObserveFixture::only_row(&fixture);

        assert_eq!(row.state(), SessionState::Stopped);
        assert!(!row.attached());
    }

    #[test]
    fn observe_copies_the_attached_flag_of_the_agent_window() {
        let session = ServiceFixture::session("fix-auth");
        let shown = ServiceFixture::observed(&session, AgentPane::live(0).shown_by_a_client());
        let fixture = ServiceFixture::new()
            .with_sessions(&[session])
            .with_multiplexer(FakeMultiplexer::new().with_live(shown));

        assert!(ObserveFixture::only_row(&fixture).attached());
    }

    #[test]
    fn a_paired_session_is_classified_from_its_agent_pane() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::exited(&session)));

        assert_eq!(
            ObserveFixture::only_row(&fixture).state(),
            SessionState::Exited
        );
    }

    #[test]
    fn an_unknown_live_session_is_adopted_with_its_tmux_name_and_creation_time() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session)));

        let row = ObserveFixture::only_row(&fixture);

        let adopted = &fixture.registry().sessions()[0];
        assert_eq!(row.state(), SessionState::Waiting);
        assert_eq!(adopted.name().as_str(), "fix-auth");
        assert_eq!(adopted.created_at(), ServiceFixture::CREATED_AT);
    }

    #[test]
    fn malformed_metadata_is_unmanaged_and_never_persisted() {
        let stranger = ServiceFixture::session("mystery");
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new().with_live(ServiceFixture::waiting(&stranger).without_metadata()),
        );

        let row = ObserveFixture::only_row(&fixture);

        assert_eq!(row.state(), SessionState::Unmanaged);
        assert_eq!(fixture.registry().save_count(), 0);
    }

    #[test]
    fn metadata_outside_the_worktree_root_is_unmanaged() {
        let stranger = ServiceFixture::session("mystery");
        let elsewhere = crate::domain::SessionMetadata::new(
            crate::domain::SessionRepo::new(stranger.repo_name().clone(), stranger.repo_path()),
            crate::domain::SessionWorktree::new(
                std::path::Path::new("/elsewhere/mystery"),
                stranger.branch(),
                "",
            ),
            "claude",
        );
        let observation = ServiceFixture::waiting(&stranger)
            .without_metadata()
            .with_metadata(elsewhere);
        let fixture =
            ServiceFixture::new().with_multiplexer(FakeMultiplexer::new().with_live(observation));

        assert_eq!(
            ObserveFixture::only_row(&fixture).state(),
            SessionState::Unmanaged
        );
    }

    #[test]
    fn metadata_with_a_foreign_branch_is_unmanaged() {
        let stranger = ServiceFixture::session("mystery");
        let foreign = stranger.metadata().with_branch("feature/mystery");
        let observation = ServiceFixture::waiting(&stranger)
            .without_metadata()
            .with_metadata(foreign);
        let fixture =
            ServiceFixture::new().with_multiplexer(FakeMultiplexer::new().with_live(observation));

        assert_eq!(
            ObserveFixture::only_row(&fixture).state(),
            SessionState::Unmanaged
        );
    }

    #[test]
    fn a_live_session_without_the_prefix_is_not_listed() {
        let stranger = ServiceFixture::session("mystery");
        let observation = SessionObservation::new(
            crate::domain::TmuxAddress::new("work", stranger.socket_path()),
            0,
        );
        let fixture =
            ServiceFixture::new().with_multiplexer(FakeMultiplexer::new().with_live(observation));

        assert!(ObserveFixture::observe(&fixture).is_empty());
    }

    #[test]
    fn an_entry_whose_tmux_name_is_live_without_metadata_shows_one_unmanaged_row() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_multiplexer(
                FakeMultiplexer::new()
                    .with_live(ServiceFixture::waiting(&session).without_metadata()),
            );

        let row = ObserveFixture::only_row(&fixture);

        assert_eq!(row.state(), SessionState::Unmanaged);
        assert_eq!(row.name(), "fix-auth");
        assert!(
            row.unmanaged_session()
                .unwrap()
                .advice()
                .unwrap()
                .contains("kill-session")
        );
    }

    #[test]
    fn observe_without_persist_writes_nothing() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session)));

        fixture.service().observe(false).unwrap();

        assert_eq!(fixture.registry().save_count(), 0);
    }

    #[test]
    fn observe_writes_only_when_reconciliation_changed_something() {
        let fixture =
            ServiceFixture::new().with_registry(FakeSessionRegistry::with_sessions(vec![
                ServiceFixture::session("fix-auth"),
            ]));

        ObserveFixture::observe(&fixture);

        assert_eq!(fixture.registry().save_count(), 0);
    }

    #[test]
    fn an_adoption_is_refused_when_an_entry_already_claims_the_worktree() {
        let entry = ServiceFixture::session("fix-auth");
        let twin = ServiceFixture::session("twin");
        let claiming = twin.metadata().clone();
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&entry))
            .with_multiplexer(
                FakeMultiplexer::new().with_live(
                    ServiceFixture::waiting(&twin)
                        .without_metadata()
                        .with_metadata(SessionMetadataFixture::claiming(&claiming, &entry)),
                ),
            );

        let rows = ObserveFixture::observe(&fixture);

        let twin_row = rows.iter().find(|row| row.name() == "twin").unwrap();
        assert_eq!(twin_row.state(), SessionState::Unmanaged);
    }

    struct SessionMetadataFixture;

    impl SessionMetadataFixture {
        fn claiming(metadata: &SessionMetadata, entry: &Session) -> SessionMetadata {
            SessionMetadata::new(
                crate::domain::SessionRepo::new(metadata.repo_name().clone(), metadata.repo_path()),
                crate::domain::SessionWorktree::new(entry.worktree_path(), metadata.branch(), ""),
                metadata.agent(),
            )
        }
    }
}

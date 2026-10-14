use super::ports::HostProbe;
use super::reconciliation::Reconciliation;
use super::worktree_containment::WorktreeContainment;
use crate::domain::{
    Config, NamingScheme, PairMismatch, Registration, Session, SessionClassifier, SessionMetadata,
    SessionName, SessionObservation, SessionState, SessionSummary, UnmanagedSession,
};

pub struct SessionReconciler<'a> {
    config: &'a Config,
    host: &'a dyn HostProbe,
    classifier: SessionClassifier,
    containment: WorktreeContainment<'a>,
    now: u64,
}

impl<'a> SessionReconciler<'a> {
    pub fn new(config: &'a Config, host: &'a dyn HostProbe, now: u64) -> Self {
        Self {
            config,
            host,
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
        let mut consumed = vec![false; observations.len()];
        for (index, entry) in entries.iter().enumerate() {
            let namesake = observations
                .iter()
                .position(|observation| observation.tmux_name() == entry.tmux_name());
            if let Some(found) = namesake {
                consumed[found] = true;
                reconciliation.add_row(self.paired_row(entry, &observations[found]));
            } else if let Some((found, rekeyed)) = self.take_over(entry, entries, &observations) {
                consumed[found] = true;
                reconciliation.add_row(self.managed_row(rekeyed.clone(), &observations[found]));
                reconciliation.rekey(index, rekeyed);
            } else {
                reconciliation.add_row(SessionSummary::managed(
                    entry.clone(),
                    SessionState::Stopped,
                    false,
                ));
            }
        }
        for (index, observation) in observations.iter().enumerate() {
            if !consumed[index] {
                self.place_unregistered(&mut reconciliation, observation, entries);
            }
        }
        reconciliation.record_observations(observations);
        reconciliation
    }

    fn paired_row(&self, entry: &Session, observation: &SessionObservation) -> SessionSummary {
        match self.pair_mismatch(entry, observation) {
            None => self.managed_row(entry.clone(), observation),
            Some(mismatch) => SessionSummary::unmanaged(UnmanagedSession::shadowing(
                entry.clone(),
                observation.clone(),
                mismatch,
            )),
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

    fn take_over(
        &self,
        entry: &Session,
        entries: &[Session],
        observations: &[SessionObservation],
    ) -> Option<(usize, Session)> {
        let claimants: Vec<usize> = (0..observations.len())
            .filter(|&index| self.claims_entry(&observations[index], entry))
            .collect();
        let [found] = claimants.as_slice() else {
            return None;
        };
        let observation = &observations[*found];
        let metadata = observation.metadata()?;
        let others: Vec<&Session> = entries
            .iter()
            .filter(|other| other.name() != entry.name())
            .collect();
        let named_elsewhere = others
            .iter()
            .any(|other| other.tmux_name() == observation.tmux_name());
        let claims_another = others
            .iter()
            .any(|other| other.worktree_path() == metadata.worktree_path());
        if named_elsewhere || claims_another {
            return None;
        }
        let name = self.take_over_name(entry, observation)?;
        if others.iter().any(|other| other.name() == &name) {
            return None;
        }
        Some((
            *found,
            entry.rekeyed(name, metadata.clone(), observation.tmux().clone()),
        ))
    }

    fn take_over_name(
        &self,
        entry: &Session,
        observation: &SessionObservation,
    ) -> Option<SessionName> {
        let tmux_name = observation.tmux_name();
        let stripped = entry
            .naming()
            .strip_tmux_prefix(tmux_name)
            .or_else(|| self.config.naming().strip_tmux_prefix(tmux_name))?;
        SessionName::from_slug(stripped).ok()
    }

    fn claims_entry(&self, observation: &SessionObservation, entry: &Session) -> bool {
        observation.tmux_name() != entry.tmux_name()
            && self
                .valid_metadata(observation, entry.naming())
                .is_some_and(|metadata| metadata.worktree_path() == entry.worktree_path())
    }

    fn place_unregistered(
        &self,
        reconciliation: &mut Reconciliation,
        observation: &SessionObservation,
        entries: &[Session],
    ) {
        match self.adoption(reconciliation.entries(), observation) {
            Some(adopted) => {
                let row = self.managed_row(adopted.clone(), observation);
                reconciliation.adopt(adopted, row);
            }
            None => reconciliation.add_row(SessionSummary::unmanaged(
                self.unmanaged(observation, entries),
            )),
        }
    }

    fn unmanaged(&self, observation: &SessionObservation, entries: &[Session]) -> UnmanagedSession {
        let naming = self.config.naming();
        let label = naming
            .strip_tmux_prefix(observation.tmux_name())
            .or_else(|| self.recorded_prefix_label(observation, entries));
        match label {
            Some(label) => UnmanagedSession::new(label, observation.clone()),
            None => self.unprefixed(observation),
        }
    }

    fn unprefixed(&self, observation: &SessionObservation) -> UnmanagedSession {
        let naming = self.config.naming();
        let unprefixed = UnmanagedSession::unprefixed(observation.clone());
        match self.valid_metadata(observation, naming) {
            Some(_) => unprefixed.missing_only(naming.tmux_prefix()),
            None => unprefixed,
        }
    }

    fn recorded_prefix_label<'o>(
        &self,
        observation: &'o SessionObservation,
        entries: &[Session],
    ) -> Option<&'o str> {
        entries
            .iter()
            .filter(|entry| self.claims_entry(observation, entry))
            .find_map(|entry| entry.naming().strip_tmux_prefix(observation.tmux_name()))
    }

    fn adoption(&self, entries: &[Session], observation: &SessionObservation) -> Option<Session> {
        let stripped = self
            .config
            .naming()
            .strip_tmux_prefix(observation.tmux_name())?;
        let name = SessionName::from_slug(stripped).ok()?;
        let metadata = self.valid_metadata(observation, self.config.naming())?;
        if entries
            .iter()
            .any(|entry| self.blocks_adoption(entry, &name, metadata))
        {
            return None;
        }
        let registration = Registration::new(
            observation.tmux().clone(),
            self.recorded_naming(),
            observation.created_at(),
        );
        Some(Session::new(name, metadata.clone(), registration))
    }

    fn blocks_adoption(
        &self,
        entry: &Session,
        name: &SessionName,
        metadata: &SessionMetadata,
    ) -> bool {
        let same_branch_in_repo = entry.branch() == metadata.branch()
            && self.host.canonical_or_given(entry.repo_path())
                == self.host.canonical_or_given(metadata.repo_path());
        entry.name() == name
            || entry.worktree_path() == metadata.worktree_path()
            || same_branch_in_repo
    }

    fn recorded_naming(&self) -> NamingScheme {
        let naming = self.config.naming();
        NamingScheme::new(
            &self.host.canonical_or_given(naming.worktree_root()),
            naming.tmux_prefix(),
        )
    }

    fn valid_metadata<'o>(
        &self,
        observation: &'o SessionObservation,
        naming: &NamingScheme,
    ) -> Option<&'o SessionMetadata> {
        observation.metadata().filter(|metadata| {
            metadata.has_absolute_paths()
                && self
                    .containment
                    .contains(naming.worktree_root(), metadata.worktree_path())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ServiceError;
    use crate::domain::{AgentPane, NamingScheme, Registration, SessionMetadata};
    use crate::test_support::{FakeMultiplexer, FakeSessionRegistry, ServiceFixture};

    struct ObserveFixture;

    impl ObserveFixture {
        const FOREIGN_BRANCH: &str = "feature/mystery";

        fn observe(fixture: &ServiceFixture) -> Vec<SessionSummary> {
            fixture.service().observe(true).unwrap()
        }

        fn only_row(fixture: &ServiceFixture) -> SessionSummary {
            let mut rows = Self::observe(fixture);
            assert_eq!(rows.len(), 1);
            rows.pop().unwrap()
        }

        fn named(tmux_name: &str) -> SessionObservation {
            SessionObservation::new(
                crate::domain::TmuxAddress::new(
                    tmux_name,
                    std::path::Path::new(FakeMultiplexer::SOCKET),
                ),
                0,
            )
        }

        fn on_foreign_branch() -> ServiceFixture {
            let stranger = ServiceFixture::session("mystery");
            let foreign = stranger.metadata().with_branch(Self::FOREIGN_BRANCH);
            let observation = ServiceFixture::waiting(&stranger)
                .without_metadata()
                .with_metadata(foreign);
            let workspace = ServiceFixture::workspace_for(&[]).with_branch(
                std::path::Path::new(ServiceFixture::REPO),
                Self::FOREIGN_BRANCH,
                ServiceFixture::TIP,
            );
            ServiceFixture::new()
                .with_workspace(workspace)
                .with_multiplexer(FakeMultiplexer::new().with_live(observation))
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
    fn metadata_with_a_foreign_branch_is_adopted() {
        let fixture = ObserveFixture::on_foreign_branch();

        assert_eq!(
            ObserveFixture::only_row(&fixture).state(),
            SessionState::Waiting
        );
    }

    #[test]
    fn an_adopted_session_on_a_foreign_branch_refuses_delete() {
        let fixture = ObserveFixture::on_foreign_branch();

        let error = fixture
            .service()
            .delete_session("mystery", &crate::application::DeleteConfirmation::force())
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("feature/mystery has no Ringleader marker")
        );
    }

    #[test]
    fn an_adopted_session_on_a_foreign_branch_refuses_rename() {
        let fixture = ObserveFixture::on_foreign_branch();

        let error = fixture
            .service()
            .rename_session("mystery", "riddle")
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("feature/mystery has no Ringleader marker")
        );
    }

    #[test]
    fn a_live_session_without_the_prefix_lists_as_unmanaged_under_its_exact_name() {
        let fixture = ServiceFixture::new()
            .with_multiplexer(FakeMultiplexer::new().with_live(ObserveFixture::named("work")));

        let row = ObserveFixture::only_row(&fixture);

        assert_eq!(row.name(), "=work");
        assert_eq!(row.state(), SessionState::Unmanaged);
    }

    #[test]
    fn a_session_made_by_hand_without_metadata_notes_the_missing_metadata() {
        let fixture = ServiceFixture::new()
            .with_multiplexer(FakeMultiplexer::new().with_live(ObserveFixture::named("scratch")));

        let row = ObserveFixture::only_row(&fixture);

        assert_eq!(row.name(), "=scratch");
        assert_eq!(
            row.unmanaged_session().unwrap().note(),
            "scratch has no valid Ringleader metadata, so it can only be attached."
        );
    }

    #[test]
    fn a_session_renamed_by_hand_without_the_prefix_notes_the_missing_prefix() {
        let session = ServiceFixture::session("fix-auth");
        let renamed = ObserveFixture::named("fixauth").with_metadata(session.metadata().clone());
        let fixture =
            ServiceFixture::new().with_multiplexer(FakeMultiplexer::new().with_live(renamed));

        let row = ObserveFixture::only_row(&fixture);

        assert_eq!(row.name(), "=fixauth");
        assert_eq!(
            row.unmanaged_session().unwrap().note(),
            "fixauth lacks the ringleader- prefix of Ringleader's sessions, so it can only be attached."
        );
    }

    #[test]
    fn a_prefixed_name_that_is_no_slug_lists_as_unmanaged() {
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new().with_live(ObserveFixture::named("ringleader-Bad_Name")),
        );

        assert_eq!(
            ObserveFixture::only_row(&fixture).state(),
            SessionState::Unmanaged
        );
    }

    #[test]
    fn delete_refuses_both_kinds_of_unregistered_session() {
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new()
                .with_live(ObserveFixture::named("work"))
                .with_live(ObserveFixture::named("ringleader-Bad_Name")),
        );
        let mut service = fixture.service();

        for name in ["=work", "Bad_Name"] {
            let error = service.delete_impact(name).unwrap_err();

            assert!(matches!(error, ServiceError::Conflict(_)), "{name}");
        }
        assert!(fixture.multiplexer().state_changing_calls().is_empty());
    }

    #[test]
    fn an_unregistered_namesake_lists_beside_the_managed_session_and_attach_picks_the_managed_one()
    {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_multiplexer(
                FakeMultiplexer::new()
                    .with_live(ServiceFixture::waiting(&session))
                    .with_live(ObserveFixture::named("fix-auth")),
            );
        let mut service = fixture.service();

        let names: Vec<String> = service
            .observe(false)
            .unwrap()
            .iter()
            .map(|row| row.name().to_string())
            .collect();
        service.attach("fix-auth").unwrap();

        assert_eq!(names, ["fix-auth", "=fix-auth"]);
        assert_eq!(
            fixture.multiplexer().attached(),
            [crate::application::ports::AttachTarget::Agent(
                "ringleader-fix-auth".to_string()
            )]
        );
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
    fn a_claimant_of_an_entry_whose_own_name_is_live_stays_unmanaged() {
        let entry = ServiceFixture::session("fix-auth");
        let claimant = RekeyFixture::renamed(&entry, "ringleader-twin", "fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&entry))
            .with_multiplexer(
                FakeMultiplexer::new()
                    .with_live(ServiceFixture::waiting(&entry))
                    .with_live(claimant),
            );

        let rows = ObserveFixture::observe(&fixture);

        let twin = rows.iter().find(|row| row.name() == "twin").unwrap();
        assert_eq!(twin.state(), SessionState::Unmanaged);
    }

    #[test]
    fn a_live_session_renamed_by_hand_takes_over_its_entry() {
        let entry = ServiceFixture::session("fix-auth");
        let renamed = RekeyFixture::renamed(&entry, "ringleader-fix-login", "fix-login");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&entry))
            .with_multiplexer(FakeMultiplexer::new().with_live(renamed));

        let row = ObserveFixture::only_row(&fixture);

        let stored = &fixture.registry().sessions()[0];
        assert_eq!(row.state(), SessionState::Waiting);
        assert_eq!(stored.name().as_str(), "fix-login");
        assert_eq!(stored.tmux_name(), "ringleader-fix-login");
        assert_eq!(stored.branch(), "fix-login");
        assert_eq!(stored.created_at(), entry.created_at());
    }

    #[test]
    fn a_take_over_shows_the_new_name_without_writing_when_not_persisting() {
        let entry = ServiceFixture::session("fix-auth");
        let renamed = RekeyFixture::renamed(&entry, "ringleader-fix-login", "fix-login");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&entry))
            .with_multiplexer(FakeMultiplexer::new().with_live(renamed));

        let rows = fixture.service().observe(false).unwrap();

        assert_eq!(rows[0].name(), "fix-login");
        assert_eq!(rows[0].session().unwrap().branch(), "fix-login");
        assert_eq!(fixture.registry().save_count(), 0);
    }

    #[test]
    fn a_delete_of_the_taken_over_name_kills_the_renamed_session() {
        let entry = ServiceFixture::session("fix-auth");
        let renamed = RekeyFixture::renamed(&entry, "ringleader-fix-login", "fix-login");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&entry))
            .with_workspace(ServiceFixture::workspace_for(&[&entry]))
            .with_host(ServiceFixture::host_for(&[&entry]))
            .with_multiplexer(FakeMultiplexer::new().with_live(renamed));
        let repo = std::path::Path::new(ServiceFixture::REPO);
        let workspace = ServiceFixture::workspace_for(&[&entry])
            .with_branch(repo, "fix-login", ServiceFixture::TIP)
            .with_marker(repo, "fix-login");
        let fixture = fixture.with_workspace(workspace);

        fixture
            .service()
            .delete_session(
                "fix-login",
                &crate::application::DeleteConfirmation::force(),
            )
            .unwrap();

        assert!(fixture.multiplexer().live_names().is_empty());
    }

    #[test]
    fn an_entry_keeps_its_recorded_naming_after_the_config_changes() {
        let entry = RekeyFixture::under_old_naming("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&entry))
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::waiting(&entry)));

        let row = ObserveFixture::only_row(&fixture);

        assert_eq!(row.state(), SessionState::Waiting);
        assert_eq!(row.tmux_name(), "old-fix-auth");
    }

    #[test]
    fn an_entry_under_the_old_naming_is_still_deletable() {
        let entry = RekeyFixture::under_old_naming("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&entry))
            .with_workspace(ServiceFixture::workspace_for(&[&entry]))
            .with_host(ServiceFixture::host_for(&[&entry]))
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::waiting(&entry)));

        let deleted = fixture
            .service()
            .delete_session("fix-auth", &crate::application::DeleteConfirmation::force());

        assert!(deleted.is_ok());
    }

    #[test]
    fn an_unregistered_session_under_the_old_root_stays_unmanaged() {
        let old = RekeyFixture::under_old_naming("fix-auth");
        let stranger = RekeyFixture::renamed(&old, "ringleader-fix-auth", "fix-auth");
        let fixture =
            ServiceFixture::new().with_multiplexer(FakeMultiplexer::new().with_live(stranger));

        assert_eq!(
            ObserveFixture::only_row(&fixture).state(),
            SessionState::Unmanaged
        );
    }

    #[test]
    fn a_shadowed_entry_turns_stopped_once_its_namesake_is_gone() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_multiplexer(
                FakeMultiplexer::new()
                    .with_live(ServiceFixture::waiting(&session).without_metadata()),
            );
        let mut service = fixture.service();
        let refused = service.rename_session("fix-auth", "other").unwrap_err();

        fixture.multiplexer().end_session("ringleader-fix-auth");
        let rows = service.observe(true).unwrap();

        assert!(
            refused
                .to_string()
                .contains("kill-session -t '=ringleader-fix-auth:'")
        );
        assert_eq!(rows[0].state(), SessionState::Stopped);
    }

    #[test]
    fn a_namesake_naming_another_worktree_makes_the_entry_unmanaged() {
        let entry = ServiceFixture::session("fix-auth");
        let other = ServiceFixture::session("dark-mode");
        let namesake = RekeyFixture::with_tmux_name(&other, entry.tmux_name());
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&entry))
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::waiting(&namesake)));

        let row = ObserveFixture::only_row(&fixture);

        assert_eq!(row.state(), SessionState::Unmanaged);
        assert!(
            row.unmanaged_session()
                .unwrap()
                .advice()
                .unwrap()
                .contains("another worktree")
        );
    }

    #[test]
    fn a_namesake_on_another_socket_makes_the_entry_unmanaged() {
        let entry = ServiceFixture::session("fix-auth");
        let elsewhere = entry.on_socket(std::path::Path::new("/tmp/other/socket"));
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&entry))
            .with_multiplexer(
                FakeMultiplexer::new().with_live(ServiceFixture::waiting(&elsewhere)),
            );

        let row = ObserveFixture::only_row(&fixture);

        assert!(
            row.unmanaged_session()
                .unwrap()
                .advice()
                .unwrap()
                .contains("another socket")
        );
    }

    #[test]
    fn sessions_in_two_repos_with_one_branch_name_both_stay_managed() {
        let first = ServiceFixture::session("fix-auth");
        let second = RekeyFixture::in_repo("fix-auth-two", "/repos/other-project", "fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(&[first.clone(), second.clone()])
            .with_multiplexer(
                FakeMultiplexer::new()
                    .with_live(ServiceFixture::waiting(&first))
                    .with_live(ServiceFixture::waiting(&second)),
            );

        let rows = ObserveFixture::observe(&fixture);

        assert!(rows.iter().all(|row| row.state() == SessionState::Waiting));
    }

    #[test]
    fn a_live_session_on_another_entry_branch_in_the_same_repo_stays_unmanaged() {
        let entry = ServiceFixture::session("fix-auth");
        let stranger = RekeyFixture::renamed(
            &ServiceFixture::session("copycat"),
            "ringleader-copycat",
            entry.branch(),
        );
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&entry))
            .with_multiplexer(FakeMultiplexer::new().with_live(stranger));

        let rows = ObserveFixture::observe(&fixture);

        let copycat = rows.iter().find(|row| row.name() == "copycat").unwrap();
        assert_eq!(copycat.state(), SessionState::Unmanaged);
    }

    #[test]
    fn an_adoption_never_takes_a_name_an_entry_uses() {
        let entry =
            RekeyFixture::with_tmux_name(&ServiceFixture::session("fix-auth"), "custom-fix-auth");
        let stranger = ServiceFixture::session("fix-auth-elsewhere");
        let live = RekeyFixture::with_tmux_name(&stranger, "ringleader-fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&entry))
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::waiting(&live)));

        let rows = ObserveFixture::observe(&fixture);

        assert!(
            rows.iter()
                .any(|row| row.state() == SessionState::Unmanaged)
        );
        assert_eq!(fixture.registry().save_count(), 0);
    }

    #[test]
    fn a_session_with_a_missing_worktree_is_still_adopted() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_host(ServiceFixture::host_for(&[]))
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session)));

        assert_eq!(
            ObserveFixture::only_row(&fixture).state(),
            SessionState::Waiting
        );
    }

    struct RekeyFixture;

    impl RekeyFixture {
        fn renamed(entry: &Session, tmux_name: &str, branch: &str) -> SessionObservation {
            let observation = ServiceFixture::waiting(&Self::with_tmux_name(entry, tmux_name));
            observation
                .without_metadata()
                .with_metadata(entry.metadata().with_branch(branch))
        }

        fn with_tmux_name(session: &Session, tmux_name: &str) -> Session {
            session.rekeyed(
                session.name().clone(),
                session.metadata().clone(),
                crate::domain::TmuxAddress::new(tmux_name, session.socket_path()),
            )
        }

        fn under_old_naming(name: &str) -> Session {
            let naming = NamingScheme::new(std::path::Path::new("/old/root"), "old-");
            let name = SessionName::new(name).unwrap();
            let repo = crate::domain::RepoName::parse("example-project").unwrap();
            let metadata = SessionMetadata::new(
                crate::domain::SessionRepo::new(
                    repo.clone(),
                    std::path::Path::new(ServiceFixture::REPO),
                ),
                crate::domain::SessionWorktree::new(
                    &naming.worktree_path(&repo, &name),
                    &naming.branch_name(&name),
                    "",
                ),
                "claude",
            );
            let tmux = crate::domain::TmuxAddress::new(
                &naming.tmux_session_name(&name),
                std::path::Path::new(crate::test_support::FakeMultiplexer::SOCKET),
            );
            Session::new(
                name,
                metadata,
                Registration::new(tmux, naming, ServiceFixture::CREATED_AT),
            )
        }

        fn in_repo(name: &str, repo_path: &str, branch: &str) -> Session {
            let base = ServiceFixture::session(name);
            let metadata = SessionMetadata::new(
                crate::domain::SessionRepo::new(
                    crate::domain::RepoName::parse("other-project").unwrap(),
                    std::path::Path::new(repo_path),
                ),
                crate::domain::SessionWorktree::new(base.worktree_path(), branch, ""),
                "claude",
            );
            base.rekeyed(
                base.name().clone(),
                metadata,
                crate::domain::TmuxAddress::new(base.tmux_name(), base.socket_path()),
            )
        }
    }
}

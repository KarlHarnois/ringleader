use std::sync::Arc;

use super::delete_impact::{DeleteConfirmation, DeleteImpact};
use super::delete_outcome::DeleteOutcome;
use super::deletion_guards::{DeletionGuards, Measurement};
use super::ports::{HostProbe, Multiplexer, ProcessInfo, SessionRegistry, Workspace};
use super::reconciliation::ManagedRow;
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use super::worktree_inspection::WorktreeInspection;
use crate::domain::{Session, SessionState};

pub struct SessionDeletion {
    guards: DeletionGuards,
    inspection: WorktreeInspection,
    workspace: Arc<dyn Workspace>,
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
    registry: Arc<dyn SessionRegistry>,
}

impl SessionDeletion {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            guards: DeletionGuards::new(ports),
            inspection: WorktreeInspection::new(ports),
            workspace: ports.repos().workspace(),
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
            registry: ports.state().registry(),
        }
    }

    pub fn impact(&self, row: &ManagedRow) -> Result<DeleteImpact, ServiceError> {
        Ok(self.guards.measure(row)?.into_impact())
    }

    pub fn delete(
        &self,
        row: &ManagedRow,
        remaining: &[Session],
        confirmation: &DeleteConfirmation,
    ) -> Result<DeleteOutcome, ServiceError> {
        if confirmation.forgets() {
            return self.forget(row, remaining, confirmation);
        }
        let session = row.session();
        let measurement = self.guards.measure(row)?;
        confirmation.check(measurement.impact())?;
        self.guards.check_removable(session)?;
        measurement.log_recovery_point(session);
        self.stop_processes(row, &measurement)?;
        self.remove_worktree(session, &measurement)?;
        self.delete_branch(session, &measurement)?;
        self.registry.save(remaining)?;
        Ok(DeleteOutcome::deleted(measurement))
    }

    fn forget(
        &self,
        row: &ManagedRow,
        remaining: &[Session],
        confirmation: &DeleteConfirmation,
    ) -> Result<DeleteOutcome, ServiceError> {
        let working = row.state() == SessionState::Working;
        if working && !confirmation.covers_working() {
            return Err(ServiceError::Conflict(format!(
                "agent is actively working in {}, confirm it to forget the session",
                row.session().name()
            )));
        }
        if row.is_live() {
            self.multiplexer.kill_session(row.session().tmux_name())?;
        }
        self.registry.save(remaining)?;
        Ok(DeleteOutcome::forgotten(row.session()))
    }

    fn stop_processes(
        &self,
        row: &ManagedRow,
        measurement: &Measurement,
    ) -> Result<(), ServiceError> {
        let session = row.session();
        let pane_sessions = self.kill_tmux_session(row)?;
        let mut targets: Vec<u32> = self
            .host
            .session_processes(&pane_sessions)
            .iter()
            .map(ProcessInfo::pid)
            .collect();
        targets.extend(
            measurement
                .impact()
                .processes()
                .iter()
                .map(ProcessInfo::pid),
        );
        let mut remaining = self.host.terminate(&targets);
        remaining.extend(self.host.processes_in(session.worktree_path(), &[]));
        if !remaining.is_empty() {
            return Err(ServiceError::Conflict(format!(
                "processes still run in {}: {}; kept the worktree, the branch, and the session, which is now stopped",
                session.worktree_path().display(),
                ProcessInfo::listed(&remaining)
            )));
        }
        self.refuse_new_commits(session, measurement)
    }

    fn kill_tmux_session(&self, row: &ManagedRow) -> Result<Vec<u32>, ServiceError> {
        if !row.is_live() {
            return Ok(Vec::new());
        }
        let keep = "kept the worktree, the branch, and the registry entry, run ringleader doctor";
        let tmux_name = row.session().tmux_name();
        let pane_sessions = self
            .multiplexer
            .pane_pids(tmux_name)
            .map_err(|error| ServiceError::from(error).adding_context(keep))?;
        self.multiplexer
            .kill_session(tmux_name)
            .map_err(|error| ServiceError::from(error).adding_context(keep))?;
        Ok(pane_sessions)
    }

    fn refuse_new_commits(
        &self,
        session: &Session,
        measurement: &Measurement,
    ) -> Result<(), ServiceError> {
        let recount = self.workspace.unmerged_commits(
            session.repo_path(),
            session.branch(),
            session.worktree_path(),
        )?;
        let unchanged = recount.same_commits_as(measurement.commits())
            && recount.count() <= measurement.commits().count();
        if unchanged {
            return Ok(());
        }
        Err(ServiceError::Conflict(format!(
            "a commit landed after the guards counted: branch tip {} became {}, worktree HEAD {} became {}; kept the worktree and the branch",
            measurement.commits().tip_label(),
            recount.tip_label(),
            measurement.commits().head_label(),
            recount.head_label()
        )))
    }

    fn remove_worktree(
        &self,
        session: &Session,
        measurement: &Measurement,
    ) -> Result<(), ServiceError> {
        if self.inspection.listed_entry(session)?.is_none() {
            return Ok(());
        }
        let Err(error) = self
            .workspace
            .remove_worktree(session.repo_path(), session.worktree_path())
        else {
            return Ok(());
        };
        let error = ServiceError::from(error);
        if self.inspection.listed_entry(session)?.is_some() {
            return Err(error.adding_context("kept the branch and the registry entry"));
        }
        Err(error.adding_context(&format!(
            "git removed the worktree only in part, remove the leftover directory {} by hand and delete again; branch tip {}, worktree HEAD {}",
            session.worktree_path().display(),
            measurement.commits().tip_label(),
            measurement.commits().head_label()
        )))
    }

    fn delete_branch(
        &self,
        session: &Session,
        measurement: &Measurement,
    ) -> Result<(), ServiceError> {
        self.workspace
            .delete_branch(
                session.repo_path(),
                session.branch(),
                measurement.commits().tip(),
            )
            .map_err(|error| {
                ServiceError::from(error).adding_context(
                    "the worktree was already removed, so deleting again skips that step and finishes",
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::application::deletion_guards::BranchPresence;
    use crate::application::ports::{
        BranchMarker, DirtyState, MultiplexerError, UnmergedCommits, WorkspaceError, WorktreeEntry,
    };
    use crate::application::session_service::SessionService;
    use crate::domain::{AgentPane, NamingScheme, Registration, TmuxAddress};
    use crate::test_support::{
        FakeMultiplexer, FakeSessionRegistry, FakeWorkspace, ServiceFixture,
    };

    const OTHER_TIP: &str = "0badc0de0badc0de0badc0de0badc0de0badc0de";

    struct DeleteFixture;

    impl DeleteFixture {
        fn session() -> Session {
            ServiceFixture::session("fix-auth")
        }

        fn worktree() -> PathBuf {
            Self::session().worktree_path().to_path_buf()
        }

        fn repo() -> &'static Path {
            Path::new(ServiceFixture::REPO)
        }

        fn live_fixture() -> ServiceFixture {
            Self::stopped_fixture().with_multiplexer(Self::live_multiplexer())
        }

        fn live_multiplexer() -> FakeMultiplexer {
            FakeMultiplexer::new()
                .with_live(ServiceFixture::waiting(&Self::session()))
                .with_pane_pids("ringleader-fix-auth", &[4100])
        }

        fn stopped_fixture() -> ServiceFixture {
            let session = Self::session();
            ServiceFixture::new()
                .with_registry(FakeSessionRegistry::with_sessions(vec![session.clone()]))
                .with_workspace(ServiceFixture::workspace_for(&[&session]))
                .with_host(ServiceFixture::host_for(&[&session]))
        }

        fn unmarked_workspace() -> FakeWorkspace {
            let session = Self::session();
            ServiceFixture::workspace_for(&[&session])
                .without_marker(Self::repo(), session.branch())
        }

        fn marking_command() -> String {
            BranchMarker::new(Self::session().branch()).marking_command(Self::repo())
        }

        fn dirty_workspace(files: u64) -> FakeWorkspace {
            ServiceFixture::workspace_for(&[&Self::session()])
                .with_dirty(&Self::worktree(), DirtyState::present(files, Vec::new()))
        }

        fn unmerged(count: u64, tip: &str) -> UnmergedCommits {
            UnmergedCommits::new(count, Some(tip), Some(tip))
        }

        fn force(service: &mut SessionService) -> Result<DeleteOutcome, ServiceError> {
            service.delete_session("fix-auth", &DeleteConfirmation::force())
        }

        fn unconfirmed(service: &mut SessionService) -> Result<DeleteOutcome, ServiceError> {
            service.delete_session(
                "fix-auth",
                &DeleteConfirmation::confirmed(DeleteImpact::default()),
            )
        }

        fn assert_everything_kept(fixture: &ServiceFixture) {
            assert!(fixture.workspace().has_branch(Self::repo(), "fix-auth"));
            assert_eq!(fixture.registry().sessions().len(), 1);
        }

        fn git_failure() -> WorkspaceError {
            WorkspaceError::Failed {
                subcommand: "worktree".to_string(),
                status: "exit status: 255".to_string(),
                stderr: "error: failed to delete".to_string(),
            }
        }
    }

    #[test]
    fn a_clean_delete_removes_the_session_worktree_branch_and_entry() {
        let fixture = DeleteFixture::live_fixture();

        let outcome = DeleteFixture::unconfirmed(&mut fixture.service()).unwrap();

        assert_eq!(outcome.tip(), Some(ServiceFixture::TIP));
        assert!(fixture.multiplexer().live_names().is_empty());
        assert!(
            !fixture
                .workspace()
                .lists_worktree(DeleteFixture::repo(), &DeleteFixture::worktree())
        );
        assert!(
            !fixture
                .workspace()
                .has_branch(DeleteFixture::repo(), "fix-auth")
        );
        assert!(fixture.registry().sessions().is_empty());
    }

    #[test]
    fn delete_reports_the_worktree_head_when_it_differs_from_the_tip() {
        let fixture = DeleteFixture::live_fixture().with_workspace(
            ServiceFixture::workspace_for(&[&DeleteFixture::session()]).with_unmerged(
                "fix-auth",
                vec![UnmergedCommits::new(
                    0,
                    Some(ServiceFixture::TIP),
                    Some(OTHER_TIP),
                )],
            ),
        );

        let outcome = DeleteFixture::force(&mut fixture.service()).unwrap();

        assert_eq!(outcome.head(), Some(OTHER_TIP));
    }

    #[test]
    fn a_dirty_worktree_refuses_an_unconfirmed_delete() {
        let fixture =
            DeleteFixture::live_fixture().with_workspace(DeleteFixture::dirty_workspace(2));

        let error = DeleteFixture::unconfirmed(&mut fixture.service()).unwrap_err();

        assert_eq!(error.exit_code(), 3);
        assert!(
            error
                .to_string()
                .contains("2 uncommitted files will be lost")
        );
        DeleteFixture::assert_everything_kept(&fixture);
    }

    #[test]
    fn unmerged_commits_on_a_clean_worktree_refuse_an_unconfirmed_delete() {
        let fixture = DeleteFixture::live_fixture().with_workspace(
            ServiceFixture::workspace_for(&[&DeleteFixture::session()]).with_unmerged(
                "fix-auth",
                vec![DeleteFixture::unmerged(3, ServiceFixture::TIP)],
            ),
        );

        let error = DeleteFixture::unconfirmed(&mut fixture.service()).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("3 commits exist on no other ref")
        );
        DeleteFixture::assert_everything_kept(&fixture);
    }

    #[test]
    fn an_outside_process_refuses_an_unconfirmed_delete() {
        let session = DeleteFixture::session();
        let fixture = DeleteFixture::live_fixture().with_host(
            ServiceFixture::host_for(&[&session]).with_process(
                5200,
                "vim notes.md",
                session.worktree_path(),
                5200,
            ),
        );

        let error = DeleteFixture::unconfirmed(&mut fixture.service()).unwrap_err();

        assert!(error.to_string().contains("process 5200 (vim notes.md)"));
    }

    #[test]
    fn the_working_guard_refuses_activity_younger_than_the_idle_threshold() {
        let session = DeleteFixture::session();
        let fixture = DeleteFixture::stopped_fixture().with_multiplexer(
            FakeMultiplexer::new().with_live(ServiceFixture::observed(
                &session,
                AgentPane::live(ServiceFixture::NOW - ServiceFixture::IDLE_SECONDS + 1),
            )),
        );

        let error = DeleteFixture::unconfirmed(&mut fixture.service()).unwrap_err();

        assert!(error.to_string().contains("agent is actively working"));
    }

    #[test]
    fn the_working_guard_allows_activity_exactly_at_the_idle_threshold() {
        let session = DeleteFixture::session();
        let fixture = DeleteFixture::stopped_fixture().with_multiplexer(
            FakeMultiplexer::new().with_live(ServiceFixture::observed(
                &session,
                AgentPane::live(ServiceFixture::NOW - ServiceFixture::IDLE_SECONDS),
            )),
        );

        assert!(DeleteFixture::unconfirmed(&mut fixture.service()).is_ok());
    }

    #[test]
    fn a_guard_that_appears_after_confirmation_refuses_the_delete() {
        let fixture = DeleteFixture::live_fixture();
        let mut service = fixture.service();
        let seen = service.delete_impact("fix-auth").unwrap();
        let fixture = fixture.with_workspace(DeleteFixture::dirty_workspace(1));
        let mut service = fixture.service();

        let error = service
            .delete_session("fix-auth", &DeleteConfirmation::confirmed(seen))
            .unwrap_err();

        assert!(matches!(error, ServiceError::GuardsChanged { .. }));
    }

    #[test]
    fn a_count_that_grew_after_confirmation_refuses_a_session_not_confirmed_as_working() {
        let fixture =
            DeleteFixture::live_fixture().with_workspace(DeleteFixture::dirty_workspace(2));
        let seen = fixture.service().delete_impact("fix-auth").unwrap();
        let fixture = fixture.with_workspace(DeleteFixture::dirty_workspace(3));

        let error = fixture
            .service()
            .delete_session("fix-auth", &DeleteConfirmation::confirmed(seen))
            .unwrap_err();

        assert_eq!(error.exit_code(), 3);
    }

    #[test]
    fn a_count_that_grew_after_confirming_a_working_session_is_accepted() {
        let session = DeleteFixture::session();
        let fixture = DeleteFixture::stopped_fixture()
            .with_workspace(DeleteFixture::dirty_workspace(2))
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::working(&session)));
        let seen = fixture.service().delete_impact("fix-auth").unwrap();
        let fixture = fixture.with_workspace(DeleteFixture::dirty_workspace(5));

        let outcome = fixture
            .service()
            .delete_session("fix-auth", &DeleteConfirmation::confirmed(seen));

        assert!(outcome.is_ok());
    }

    #[test]
    fn delete_impact_on_a_clean_session_changes_nothing() {
        let fixture = DeleteFixture::live_fixture();

        let impact = fixture.service().delete_impact("fix-auth").unwrap();

        assert!(!impact.needs_confirmation());
        assert!(fixture.workspace().state_changing_calls().is_empty());
        assert!(fixture.multiplexer().state_changing_calls().is_empty());
        assert_eq!(fixture.registry().save_count(), 0);
    }

    #[test]
    fn delete_impact_matches_the_impact_a_refused_delete_carries() {
        let fixture = DeleteFixture::live_fixture().with_workspace(
            DeleteFixture::dirty_workspace(2).with_unmerged(
                "fix-auth",
                vec![DeleteFixture::unmerged(1, ServiceFixture::TIP)],
            ),
        );

        let impact = fixture.service().delete_impact("fix-auth").unwrap();
        let error = DeleteFixture::unconfirmed(&mut fixture.service()).unwrap_err();

        let ServiceError::GuardsChanged {
            impact: carried, ..
        } = error
        else {
            panic!("expected a guard refusal, got {error}");
        };
        assert_eq!(*carried, impact);
    }

    #[test]
    fn delete_refuses_a_worktree_outside_the_recorded_root() {
        let inside = DeleteFixture::session();
        let outside_root = NamingScheme::new(Path::new("/elsewhere"), "ringleader-");
        let session = Session::new(
            inside.name().clone(),
            inside.metadata().clone(),
            Registration::new(
                TmuxAddress::new(inside.tmux_name(), inside.socket_path()),
                outside_root,
                ServiceFixture::CREATED_AT,
            ),
        );
        let fixture = DeleteFixture::stopped_fixture().with_sessions(&[session]);

        let error = DeleteFixture::force(&mut fixture.service()).unwrap_err();

        assert!(error.to_string().contains("not inside the worktree root"));
        assert!(fixture.workspace().state_changing_calls().is_empty());
    }

    #[test]
    fn a_failed_worktree_removal_keeps_the_branch_and_entry() {
        let fixture = DeleteFixture::live_fixture().with_workspace(
            ServiceFixture::workspace_for(&[&DeleteFixture::session()])
                .failing("remove_worktree", DeleteFixture::git_failure()),
        );

        let error = DeleteFixture::force(&mut fixture.service()).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("kept the branch and the registry entry")
        );
        DeleteFixture::assert_everything_kept(&fixture);
    }

    #[test]
    fn a_partial_removal_reports_the_leftover_path_and_the_recorded_shas() {
        let fixture = DeleteFixture::live_fixture().with_workspace(
            ServiceFixture::workspace_for(&[&DeleteFixture::session()])
                .failing("remove_worktree", DeleteFixture::git_failure())
                .removing_partially(),
        );

        let message = DeleteFixture::force(&mut fixture.service())
            .unwrap_err()
            .to_string();

        assert!(message.contains(&DeleteFixture::worktree().display().to_string()));
        assert!(message.contains(ServiceFixture::TIP));
        DeleteFixture::assert_everything_kept(&fixture);
    }

    #[test]
    fn a_new_tip_after_the_kill_refuses_the_delete_and_keeps_the_branch() {
        let fixture = DeleteFixture::live_fixture().with_workspace(
            ServiceFixture::workspace_for(&[&DeleteFixture::session()]).with_unmerged(
                "fix-auth",
                vec![
                    DeleteFixture::unmerged(0, ServiceFixture::TIP),
                    DeleteFixture::unmerged(1, OTHER_TIP),
                ],
            ),
        );

        let error = DeleteFixture::force(&mut fixture.service()).unwrap_err();

        assert!(error.to_string().contains(OTHER_TIP));
        DeleteFixture::assert_everything_kept(&fixture);
    }

    #[test]
    fn a_process_that_survives_sigterm_refuses_the_delete_and_leaves_the_session_stopped() {
        let session = DeleteFixture::session();
        let fixture = DeleteFixture::live_fixture().with_host(
            ServiceFixture::host_for(&[&session]).with_stubborn_process(
                5300,
                "bash",
                session.worktree_path(),
                5300,
            ),
        );

        let error = DeleteFixture::force(&mut fixture.service()).unwrap_err();

        assert!(error.to_string().contains("5300 (bash)"));
        assert!(fixture.multiplexer().live_names().is_empty());
        DeleteFixture::assert_everything_kept(&fixture);
    }

    #[test]
    fn delete_stops_the_session_processes_and_the_confirmed_outside_processes() {
        let session = DeleteFixture::session();
        let fixture = DeleteFixture::live_fixture().with_host(
            ServiceFixture::host_for(&[&session])
                .with_process(4101, "nohup sleep", Path::new("/tmp"), 4100)
                .with_process(5200, "vim", session.worktree_path(), 5200),
        );

        DeleteFixture::force(&mut fixture.service()).unwrap();

        assert_eq!(fixture.host().terminated(), [4101, 5200]);
    }

    #[test]
    fn a_vanished_repository_refuses_delete_without_forget() {
        let fixture = DeleteFixture::live_fixture().with_workspace(
            ServiceFixture::workspace_for(&[&DeleteFixture::session()])
                .with_invalid_repo(DeleteFixture::repo()),
        );

        let error = DeleteFixture::force(&mut fixture.service()).unwrap_err();

        assert!(error.to_string().contains("--forget"));
        DeleteFixture::assert_everything_kept(&fixture);
    }

    #[test]
    fn forget_removes_only_the_registry_entry() {
        let fixture = DeleteFixture::live_fixture().with_workspace(
            ServiceFixture::workspace_for(&[&DeleteFixture::session()])
                .with_invalid_repo(DeleteFixture::repo()),
        );

        let outcome = fixture
            .service()
            .delete_session("fix-auth", &DeleteConfirmation::force().forgetting())
            .unwrap();

        assert_eq!(
            outcome.kept(),
            Some((DeleteFixture::worktree().as_path(), "fix-auth"))
        );
        assert!(fixture.workspace().state_changing_calls().is_empty());
        assert!(fixture.registry().sessions().is_empty());
        assert!(fixture.multiplexer().live_names().is_empty());
    }

    #[test]
    fn forget_works_while_ringleader_runs_inside_the_worktree() {
        let worktree = DeleteFixture::session().worktree_path().join("src");
        let fixture = DeleteFixture::live_fixture().launched_in(&worktree);

        fixture
            .service()
            .delete_session("fix-auth", &DeleteConfirmation::force().forgetting())
            .unwrap();

        assert!(fixture.registry().sessions().is_empty());
        assert!(fixture.workspace().state_changing_calls().is_empty());
    }

    #[test]
    fn measuring_a_delete_refuses_while_ringleader_runs_inside_the_worktree() {
        let worktree = DeleteFixture::session().worktree_path().join("src");
        let fixture = DeleteFixture::live_fixture().launched_in(&worktree);

        let error = fixture.service().delete_impact("fix-auth").unwrap_err();

        assert_eq!(error.exit_code(), 3);
    }

    #[test]
    fn a_worktree_git_does_not_list_refuses_before_measuring() {
        let session = DeleteFixture::session();
        let fixture =
            DeleteFixture::live_fixture().with_workspace(ServiceFixture::workspace_for(&[]));

        DeleteFixture::force(&mut fixture.service()).unwrap_err();

        let calls = fixture.workspace().calls();
        assert!(calls.iter().all(|call| !call.starts_with("dirty_state")));
        assert!(
            calls
                .iter()
                .all(|call| !call.starts_with("unmerged_commits"))
        );
        assert!(
            fixture
                .multiplexer()
                .live_names()
                .contains(&session.tmux_name().to_string())
        );
    }

    #[test]
    fn a_worktree_whose_git_file_is_missing_refuses_before_measuring() {
        let session = DeleteFixture::session();
        let fixture = DeleteFixture::live_fixture()
            .with_host(ServiceFixture::host_for(&[]).with_directory(session.worktree_path()));

        let error = DeleteFixture::force(&mut fixture.service()).unwrap_err();

        assert!(error.to_string().contains(".git is missing"));
        assert!(
            fixture
                .workspace()
                .calls()
                .iter()
                .all(|call| !call.starts_with("dirty_state"))
        );
    }

    #[test]
    fn deleting_an_orphan_session_without_commits_returns_no_sha() {
        let session = DeleteFixture::session();
        let workspace = FakeWorkspace::default()
            .with_worktree(
                DeleteFixture::repo(),
                crate::application::ports::WorktreeEntry::new(session.worktree_path())
                    .on_branch(session.branch()),
            )
            .with_marker(DeleteFixture::repo(), session.branch());
        let fixture = DeleteFixture::live_fixture().with_workspace(workspace);

        let outcome = DeleteFixture::force(&mut fixture.service()).unwrap();

        assert_eq!(outcome.tip(), None);
        assert_eq!(outcome.branch(), BranchPresence::Unborn);
    }

    #[test]
    fn a_delete_retried_after_the_branch_is_gone_completes() {
        let fixture = DeleteFixture::stopped_fixture()
            .with_workspace(ServiceFixture::workspace_for(&[]))
            .with_host(ServiceFixture::host_for(&[]));

        let outcome = DeleteFixture::force(&mut fixture.service()).unwrap();

        assert_eq!(outcome.branch(), BranchPresence::Missing);
        assert!(fixture.registry().sessions().is_empty());
    }

    #[test]
    fn a_delete_of_a_session_whose_worktree_directory_is_missing_completes() {
        let fixture = DeleteFixture::stopped_fixture().with_host(ServiceFixture::host_for(&[]));

        assert!(DeleteFixture::force(&mut fixture.service()).is_ok());
        assert!(fixture.registry().sessions().is_empty());
    }

    #[test]
    fn delete_refuses_the_session_this_process_runs_in() {
        let fixture = DeleteFixture::live_fixture()
            .with_multiplexer(DeleteFixture::live_multiplexer().with_caller("ringleader-fix-auth"));

        let error = DeleteFixture::force(&mut fixture.service()).unwrap_err();

        assert_eq!(error.exit_code(), 3);
        assert!(fixture.multiplexer().state_changing_calls().is_empty());
    }

    #[test]
    fn delete_refuses_while_ringleader_runs_inside_the_worktree() {
        let worktree = DeleteFixture::session().worktree_path().join("src");
        let fixture = DeleteFixture::live_fixture().launched_in(&worktree);

        let error = DeleteFixture::force(&mut fixture.service()).unwrap_err();

        assert_eq!(error.exit_code(), 3);
        assert!(
            error
                .to_string()
                .contains("delete it from another directory")
        );
        assert!(fixture.multiplexer().state_changing_calls().is_empty());
    }

    #[test]
    fn delete_refuses_an_unmarked_branch_with_the_marking_command() {
        let fixture =
            DeleteFixture::live_fixture().with_workspace(DeleteFixture::unmarked_workspace());

        let error = DeleteFixture::force(&mut fixture.service()).unwrap_err();

        assert_eq!(error.exit_code(), ServiceError::CONFLICT_EXIT);
        assert!(
            error
                .to_string()
                .contains(&DeleteFixture::marking_command())
        );
    }

    #[test]
    fn forget_drops_a_session_on_an_unmarked_branch() {
        let fixture =
            DeleteFixture::live_fixture().with_workspace(DeleteFixture::unmarked_workspace());

        fixture
            .service()
            .delete_session("fix-auth", &DeleteConfirmation::force().forgetting())
            .unwrap();

        assert!(fixture.registry().sessions().is_empty());
        assert!(fixture.workspace().state_changing_calls().is_empty());
    }

    #[test]
    fn a_refused_unmarked_delete_changes_nothing() {
        let fixture =
            DeleteFixture::live_fixture().with_workspace(DeleteFixture::unmarked_workspace());

        DeleteFixture::force(&mut fixture.service()).unwrap_err();

        assert!(fixture.workspace().state_changing_calls().is_empty());
        assert_eq!(fixture.multiplexer().live_names(), ["ringleader-fix-auth"]);
        DeleteFixture::assert_everything_kept(&fixture);
    }

    #[test]
    fn delete_refuses_an_unmarked_unborn_branch() {
        let session = DeleteFixture::session();
        let workspace = FakeWorkspace::default().with_worktree(
            DeleteFixture::repo(),
            WorktreeEntry::new(session.worktree_path()).on_branch(session.branch()),
        );
        let fixture = DeleteFixture::live_fixture().with_workspace(workspace);

        let error = DeleteFixture::force(&mut fixture.service()).unwrap_err();

        assert!(
            error
                .to_string()
                .contains(&DeleteFixture::marking_command())
        );
    }

    #[test]
    fn measuring_a_delete_refuses_an_unmarked_branch() {
        let fixture =
            DeleteFixture::live_fixture().with_workspace(DeleteFixture::unmarked_workspace());

        let error = fixture.service().delete_impact("fix-auth").unwrap_err();

        assert!(error.to_string().contains("has no Ringleader marker"));
    }

    #[test]
    fn a_locked_worktree_refuses_the_delete_and_leaves_the_session_running() {
        let session = DeleteFixture::session();
        let workspace = FakeWorkspace::default()
            .with_worktree(
                DeleteFixture::repo(),
                crate::application::ports::WorktreeEntry::new(session.worktree_path())
                    .on_branch(session.branch())
                    .at_head(ServiceFixture::TIP)
                    .locked(),
            )
            .with_branch(DeleteFixture::repo(), session.branch(), ServiceFixture::TIP)
            .with_marker(DeleteFixture::repo(), session.branch());
        let fixture = DeleteFixture::live_fixture().with_workspace(workspace);

        let error = DeleteFixture::force(&mut fixture.service()).unwrap_err();

        assert!(error.to_string().contains("locked"));
        assert_eq!(fixture.multiplexer().live_names(), ["ringleader-fix-auth"]);
    }

    #[test]
    fn processes_in_a_stopped_worktree_are_counted_instead_of_refusing_the_delete() {
        let session = DeleteFixture::session();
        let fixture = DeleteFixture::stopped_fixture()
            .with_host(ServiceFixture::with_worktree_process(&session));

        let impact = fixture.service().delete_impact("fix-auth").unwrap();

        assert_eq!(impact.processes().len(), 1);
    }

    #[test]
    fn a_stopped_entry_whose_worktree_a_renamed_out_live_session_claims_refuses_delete() {
        let session = DeleteFixture::session();
        let claimant = SessionObservationFixture::renamed_out(&session);
        let fixture = DeleteFixture::stopped_fixture()
            .with_multiplexer(FakeMultiplexer::new().with_live(claimant));

        let error = DeleteFixture::force(&mut fixture.service()).unwrap_err();

        assert!(error.to_string().contains("claims the worktree"));
    }

    #[test]
    fn a_timed_out_kill_keeps_the_worktree_branch_and_entry() {
        let fixture = DeleteFixture::live_fixture().with_multiplexer(
            DeleteFixture::live_multiplexer().failing(
                "kill_session",
                MultiplexerError::TimedOut {
                    subcommand: "kill-session".to_string(),
                    seconds: 10,
                },
            ),
        );

        let error = DeleteFixture::force(&mut fixture.service()).unwrap_err();

        assert!(error.to_string().contains("doctor"));
        assert!(
            fixture
                .workspace()
                .lists_worktree(DeleteFixture::repo(), &DeleteFixture::worktree())
        );
        DeleteFixture::assert_everything_kept(&fixture);
    }

    #[test]
    fn delete_still_runs_with_no_agents_configured() {
        let fixture =
            DeleteFixture::live_fixture().with_config(ServiceFixture::config().with_agents(vec![]));

        assert!(DeleteFixture::force(&mut fixture.service()).is_ok());
    }

    struct SessionObservationFixture;

    impl SessionObservationFixture {
        fn renamed_out(session: &Session) -> crate::domain::SessionObservation {
            crate::domain::SessionObservation::new(
                TmuxAddress::new("my-own-name", session.socket_path()),
                ServiceFixture::CREATED_AT,
            )
            .with_agent_pane(AgentPane::live(0))
            .with_metadata(session.metadata().clone())
        }
    }
}

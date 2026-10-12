use super::metadata_check::MetadataCheck;
use super::ports::HostProbe;
use crate::domain::{
    Config, NamingScheme, Registration, Session, SessionMetadata, SessionName, SessionObservation,
};

pub struct UnregisteredPlacement<'a> {
    config: &'a Config,
    host: &'a dyn HostProbe,
    check: MetadataCheck<'a>,
}

impl<'a> UnregisteredPlacement<'a> {
    const EXACT_LABEL: &'static str = "=";

    pub fn new(config: &'a Config, host: &'a dyn HostProbe) -> Self {
        Self {
            config,
            host,
            check: MetadataCheck::new(host),
        }
    }

    pub fn adoption(
        &self,
        entries: &[Session],
        observation: &SessionObservation,
    ) -> Option<Session> {
        let stripped = self
            .config
            .naming()
            .strip_tmux_prefix(observation.tmux_name())?;
        let name = SessionName::from_slug(stripped).ok()?;
        let metadata = self
            .check
            .valid_metadata(observation, self.config.naming())?;
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
            naming.branch_prefix(),
            naming.tmux_prefix(),
        )
    }

    pub fn unmanaged_label(&self, observation: &SessionObservation, entries: &[Session]) -> String {
        let tmux_name = observation.tmux_name();
        self.config
            .naming()
            .strip_tmux_prefix(tmux_name)
            .or_else(|| self.recorded_prefix_label(observation, entries))
            .map(str::to_string)
            .unwrap_or_else(|| format!("{}{tmux_name}", Self::EXACT_LABEL))
    }

    fn recorded_prefix_label<'o>(
        &self,
        observation: &'o SessionObservation,
        entries: &[Session],
    ) -> Option<&'o str> {
        entries
            .iter()
            .filter(|entry| self.check.claims_entry(observation, entry))
            .find_map(|entry| entry.naming().strip_tmux_prefix(observation.tmux_name()))
    }
}

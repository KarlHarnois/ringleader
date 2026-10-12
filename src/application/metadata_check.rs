use super::ports::HostProbe;
use super::worktree_containment::WorktreeContainment;
use crate::domain::{NamingScheme, Session, SessionMetadata, SessionObservation};

pub struct MetadataCheck<'a> {
    containment: WorktreeContainment<'a>,
}

impl<'a> MetadataCheck<'a> {
    pub fn new(host: &'a dyn HostProbe) -> Self {
        Self {
            containment: WorktreeContainment::new(host),
        }
    }

    pub fn valid_metadata<'o>(
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

    pub fn claims_entry(&self, observation: &SessionObservation, entry: &Session) -> bool {
        observation.tmux_name() != entry.tmux_name()
            && self
                .valid_metadata(observation, entry.naming())
                .is_some_and(|metadata| metadata.worktree_path() == entry.worktree_path())
    }
}

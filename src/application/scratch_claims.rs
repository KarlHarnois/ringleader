use std::collections::BTreeSet;
use std::path::PathBuf;

use super::ports::ScratchSpace;
use crate::domain::{Session, SessionMetadata, SessionObservation};

pub struct ScratchClaims {
    claimed: BTreeSet<PathBuf>,
}

impl ScratchClaims {
    pub fn new(
        space: &dyn ScratchSpace,
        entries: &[Session],
        observations: &[SessionObservation],
    ) -> Self {
        let recorded = entries.iter().map(Session::worktree_path);
        let observed = observations
            .iter()
            .filter_map(SessionObservation::metadata)
            .map(SessionMetadata::worktree_path);
        let claimed = recorded
            .chain(observed)
            .map(|worktree| space.directory(worktree))
            .collect();
        Self { claimed }
    }

    pub fn orphans(&self, directories: Vec<PathBuf>) -> Vec<PathBuf> {
        directories
            .into_iter()
            .filter(|directory| !self.claimed.contains(directory))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FakeScratchSpace, ServiceFixture};

    struct ClaimsFixture {
        space: FakeScratchSpace,
    }

    impl ClaimsFixture {
        fn new() -> Self {
            Self {
                space: FakeScratchSpace::default(),
            }
        }

        fn directory_of(&self, name: &str) -> PathBuf {
            self.space.directory(&ServiceFixture::worktree_path(name))
        }

        fn orphans_among(
            &self,
            names: &[&str],
            entries: &[Session],
            observations: &[SessionObservation],
        ) -> Vec<PathBuf> {
            let directories = names.iter().map(|name| self.directory_of(name)).collect();
            ScratchClaims::new(&self.space, entries, observations).orphans(directories)
        }
    }

    #[test]
    fn a_stopped_session_keeps_its_directory_so_it_can_resume() {
        let fixture = ClaimsFixture::new();
        let stopped = ServiceFixture::session("fix-auth");

        let orphans = fixture.orphans_among(&["fix-auth"], &[stopped], &[]);

        assert!(orphans.is_empty());
    }

    #[test]
    fn a_live_tmux_session_outside_the_registry_keeps_its_directory() {
        let fixture = ClaimsFixture::new();
        let unrecorded = ServiceFixture::waiting(&ServiceFixture::session("fix-auth"));

        let orphans = fixture.orphans_among(&["fix-auth"], &[], &[unrecorded]);

        assert!(orphans.is_empty());
    }

    #[test]
    fn a_directory_that_nothing_claims_is_an_orphan() {
        let fixture = ClaimsFixture::new();
        let recorded = ServiceFixture::session("fix-auth");

        let orphans = fixture.orphans_among(&["fix-auth", "forgotten"], &[recorded], &[]);

        assert_eq!(orphans, [fixture.directory_of("forgotten")]);
    }
}

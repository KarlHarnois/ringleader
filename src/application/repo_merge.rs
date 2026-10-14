use std::path::Path;

use super::ports::HostProbe;
use crate::domain::{BaseBranch, Branch, InvalidName, Repo, RepoName, RepoScope, Session};

pub struct RepoMerge<'a> {
    host: &'a dyn HostProbe,
}

impl<'a> RepoMerge<'a> {
    pub fn new(host: &'a dyn HostProbe) -> Self {
        Self { host }
    }

    pub fn merge(&self, scope: &RepoScope, scanned: &[Repo], explicit: &[Repo]) -> Vec<Repo> {
        let mut merged: Vec<Repo> = explicit.iter().map(|repo| self.canonical(repo)).collect();
        let enclosing = scope
            .enclosing_repo()
            .and_then(|top_level| Self::discovered(top_level).ok());
        for repo in enclosing.iter().chain(scanned) {
            if merged.iter().all(|kept| kept.path() != repo.path()) {
                merged.push(repo.clone());
            }
        }
        merged.sort_by(|first, second| {
            (first.name(), first.path()).cmp(&(second.name(), second.path()))
        });
        merged
    }

    pub fn resolved(&self, top_level: &Path, explicit: &[Repo]) -> Result<Repo, InvalidName> {
        let top_level = self.host.canonical_or_given(top_level);
        match self.configured(&top_level, explicit) {
            Some(repo) => Ok(repo.at_path(&top_level)),
            None => Self::discovered(&top_level),
        }
    }

    pub fn configured<'e>(&self, top_level: &Path, explicit: &'e [Repo]) -> Option<&'e Repo> {
        explicit
            .iter()
            .find(|repo| self.host.canonical_or_given(repo.path()) == top_level)
    }

    pub fn base_branch_of<'s>(
        &self,
        session: &'s Session,
        explicit: &'s [Repo],
    ) -> Option<&'s BaseBranch> {
        session
            .requested_base_branch()
            .or_else(|| self.configured_base_branch(session, explicit))
    }

    pub fn session_branches_in(&self, repo: &Path, sessions: &[Session]) -> Vec<Branch> {
        let repo = self.host.canonical_or_given(repo);
        sessions
            .iter()
            .filter(|session| self.host.canonical_or_given(session.repo_path()) == repo)
            .map(|session| Branch::Local(session.branch().to_string()))
            .collect()
    }

    fn configured_base_branch<'e>(
        &self,
        session: &Session,
        explicit: &'e [Repo],
    ) -> Option<&'e BaseBranch> {
        let repo_path = self.host.canonical_or_given(session.repo_path());
        self.configured(&repo_path, explicit)
            .and_then(Repo::base_branch)
    }

    fn canonical(&self, repo: &Repo) -> Repo {
        repo.at_path(&self.host.canonical_or_given(repo.path()))
    }

    fn discovered(top_level: &Path) -> Result<Repo, InvalidName> {
        Ok(Repo::discovered(
            RepoName::of_directory(top_level)?,
            top_level,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FakeHostProbe, ServiceFixture};

    #[test]
    fn the_configured_entry_is_found_by_its_canonical_path_and_kept_as_written() {
        let host = FakeHostProbe::default().with_canonical(
            Path::new("/link/example-project"),
            Path::new("/work/example-project"),
        );
        let entry = Repo::explicit(
            RepoName::parse("pinned").unwrap(),
            Path::new("/link/example-project"),
            None,
        );
        let explicit = [entry.clone()];

        let configured =
            RepoMerge::new(&host).configured(Path::new("/work/example-project"), &explicit);

        assert_eq!(configured, Some(&entry));
    }

    #[test]
    fn only_the_sessions_of_the_repo_give_their_local_branches() {
        let here = ServiceFixture::session("main");
        let elsewhere =
            ServiceFixture::session_at("master", "/repos/other", ServiceFixture::CREATED_AT);
        let host = FakeHostProbe::default();

        assert_eq!(
            RepoMerge::new(&host).session_branches_in(here.repo_path(), &[here.clone(), elsewhere]),
            [Branch::Local("main".to_string())]
        );
    }

    #[test]
    fn a_session_recorded_through_a_symlink_shares_the_repo() {
        let linked =
            ServiceFixture::session_at("main", "/link/example-project", ServiceFixture::CREATED_AT);
        let host = FakeHostProbe::default().with_canonical(
            Path::new("/link/example-project"),
            Path::new("/work/example-project"),
        );

        assert_eq!(
            RepoMerge::new(&host)
                .session_branches_in(Path::new("/work/example-project"), &[linked]),
            [Branch::Local("main".to_string())]
        );
    }
}

use std::path::Path;

use super::session::Session;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Branch {
    Local(String),
    RemoteTracking(String),
}

impl Branch {
    pub const ORIGIN: &'static str = "origin";

    pub fn origin_copy_of(name: &str) -> Self {
        Branch::RemoteTracking(format!("{}/{name}", Self::ORIGIN))
    }

    pub fn of_sessions_in(repo: &Path, sessions: &[Session]) -> Vec<Self> {
        sessions
            .iter()
            .filter(|session| session.repo_path() == repo)
            .map(|session| Branch::Local(session.branch().to_string()))
            .collect()
    }

    pub fn name(&self) -> &str {
        match self {
            Branch::Local(name) | Branch::RemoteTracking(name) => name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ServiceFixture;

    #[test]
    fn the_origin_copy_of_a_branch_is_tracked_under_origin() {
        assert_eq!(
            Branch::origin_copy_of("main"),
            Branch::RemoteTracking("origin/main".to_string())
        );
    }

    #[test]
    fn only_the_sessions_of_the_repo_give_their_local_branches() {
        let here = ServiceFixture::session("main");
        let elsewhere =
            ServiceFixture::session_at("master", "/repos/other", ServiceFixture::CREATED_AT);

        assert_eq!(
            Branch::of_sessions_in(here.repo_path(), &[here.clone(), elsewhere]),
            [Branch::Local("main".to_string())]
        );
    }

    #[test]
    fn a_branch_is_named_as_users_type_it() {
        let origin_copy = Branch::origin_copy_of("develop");

        assert_eq!(origin_copy.name(), "origin/develop");
    }
}

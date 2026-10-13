use std::fmt;

use crate::domain::{RepoName, SessionSummary};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RepoGroup {
    Named(RepoName),
    Other,
}

impl RepoGroup {
    const OTHER: &'static str = "other";

    pub fn of(row: &SessionSummary) -> Self {
        row.metadata().map_or(Self::Other, |metadata| {
            Self::Named(metadata.repo_name().clone())
        })
    }
}

impl fmt::Display for RepoGroup {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named(name) => write!(formatter, "{name}"),
            Self::Other => formatter.write_str(Self::OTHER),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{SessionState, UnmanagedSession};
    use crate::test_support::ServiceFixture;

    struct GroupFixture;

    impl GroupFixture {
        fn named(name: &str) -> RepoGroup {
            RepoGroup::Named(RepoName::parse(name).unwrap())
        }
    }

    #[test]
    fn named_repos_sort_alphabetically_before_other() {
        let mut groups = vec![
            RepoGroup::Other,
            GroupFixture::named("zebra"),
            GroupFixture::named("alpha"),
        ];

        groups.sort();

        assert_eq!(
            groups,
            [
                GroupFixture::named("alpha"),
                GroupFixture::named("zebra"),
                RepoGroup::Other
            ]
        );
    }

    #[test]
    fn a_row_belongs_to_the_repo_of_its_metadata() {
        let row = ServiceFixture::summary_in("fix-auth", "/repos/alpha", SessionState::Waiting);

        assert_eq!(RepoGroup::of(&row).to_string(), "alpha");
    }

    #[test]
    fn a_row_without_metadata_is_other() {
        let stranger = ServiceFixture::session("mystery");
        let row = SessionSummary::unmanaged(UnmanagedSession::new(
            "mystery",
            ServiceFixture::waiting(&stranger).without_metadata(),
        ));

        assert_eq!(RepoGroup::of(&row), RepoGroup::Other);
        assert_eq!(RepoGroup::Other.to_string(), "other");
    }
}

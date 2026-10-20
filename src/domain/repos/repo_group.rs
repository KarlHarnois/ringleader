use std::fmt;
use std::path::{Path, PathBuf};

use super::repo_name::RepoName;
use crate::domain::SessionSummary;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RepoGroup {
    Named(RepoName, PathBuf),
    Other,
}

impl RepoGroup {
    const OTHER: &'static str = "other";

    pub fn of(row: &SessionSummary) -> Self {
        row.metadata().map_or(Self::Other, |metadata| {
            Self::Named(
                metadata.repo_name().clone(),
                metadata.repo_path().to_path_buf(),
            )
        })
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Named(_, path) => Some(path),
            Self::Other => None,
        }
    }
}

impl fmt::Display for RepoGroup {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named(name, _) => write!(formatter, "{name}"),
            Self::Other => formatter.write_str(Self::OTHER),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{SessionState, UnmanagedSession};
    use crate::test_support::ServiceFixture;

    #[test]
    fn named_repos_sort_alphabetically_before_other() {
        let mut groups = vec![
            RepoGroup::Other,
            ServiceFixture::repo_group("/repos/zebra"),
            ServiceFixture::repo_group("/repos/alpha"),
        ];

        groups.sort();

        assert_eq!(
            groups,
            [
                ServiceFixture::repo_group("/repos/alpha"),
                ServiceFixture::repo_group("/repos/zebra"),
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
    fn repos_sharing_a_name_are_separate_groups_with_the_same_label() {
        let work = ServiceFixture::summary_in("a-one", "/work/api", SessionState::Waiting);
        let fork = ServiceFixture::summary_in("b-two", "/forks/api", SessionState::Waiting);

        assert_ne!(RepoGroup::of(&work), RepoGroup::of(&fork));
        assert_eq!(RepoGroup::of(&fork).to_string(), "api");
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

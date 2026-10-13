use super::repo_group::RepoGroup;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListSelection {
    Session(String),
    Repo(RepoGroup),
}

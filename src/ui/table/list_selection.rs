use crate::domain::RepoGroup;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListSelection {
    Session(String),
    Repo(RepoGroup),
}

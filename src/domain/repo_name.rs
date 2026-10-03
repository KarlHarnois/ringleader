use std::fmt;

use super::session_name::{InvalidName, Slug};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RepoName(Slug);

impl RepoName {
    pub fn from_basename(basename: &str) -> Result<Self, InvalidName> {
        Slug::from_text(basename).map(Self)
    }

    pub fn parse(text: &str) -> Result<Self, InvalidName> {
        Slug::from_exact(text).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for RepoName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_a_directory_basename() {
        let name = RepoName::from_basename("Example_Project").unwrap();

        assert_eq!(name.as_str(), "example-project");
    }

    #[test]
    fn rejects_a_basename_that_slugs_to_empty() {
        assert!(RepoName::from_basename("...").is_err());
    }

    #[test]
    fn parse_rejects_a_path_separator() {
        assert!(RepoName::parse("a/b").is_err());
    }

    #[test]
    fn parse_rejects_a_parent_directory_segment() {
        assert!(RepoName::parse("..").is_err());
    }

    #[test]
    fn parse_accepts_a_slug() {
        assert_eq!(
            RepoName::parse("example-project").unwrap().as_str(),
            "example-project"
        );
    }
}

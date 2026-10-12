use std::path::Path;

pub struct BranchMarker<'a> {
    branch: &'a str,
}

impl<'a> BranchMarker<'a> {
    pub const VARIABLE: &'static str = "ringleaderManaged";
    pub const VALUE: &'static str = "true";
    const SECTION: &'static str = "branch";

    pub fn new(branch: &'a str) -> Self {
        Self { branch }
    }

    pub fn from_key(key: &'a str) -> Option<Self> {
        let (section, variable) = key.rsplit_once('.')?;
        let branch = section.strip_prefix(Self::SECTION)?.strip_prefix('.')?;
        variable
            .eq_ignore_ascii_case(Self::VARIABLE)
            .then_some(Self { branch })
    }

    pub fn listing_pattern() -> String {
        format!(
            "^{}\\..*\\.{}$",
            Self::SECTION,
            Self::VARIABLE.to_ascii_lowercase()
        )
    }

    pub fn branch(&self) -> &'a str {
        self.branch
    }

    pub fn key(&self) -> String {
        format!("{}.{}.{}", Self::SECTION, self.branch, Self::VARIABLE)
    }

    pub fn marking_command(&self, repo: &Path) -> String {
        format!(
            "git -C {} config {} {}",
            repo.display(),
            self.key(),
            Self::VALUE
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_names_the_branch_section() {
        assert_eq!(
            BranchMarker::new("fix-auth").key(),
            "branch.fix-auth.ringleaderManaged"
        );
    }

    #[test]
    fn the_marking_command_sets_the_key_in_the_repo() {
        let command = BranchMarker::new("fix-auth").marking_command(Path::new("/repos/example"));

        assert_eq!(
            command,
            "git -C /repos/example config branch.fix-auth.ringleaderManaged true"
        );
    }

    #[test]
    fn a_listed_key_names_its_branch_with_dots_and_case_kept() {
        let marker = BranchMarker::from_key("branch.Feature/v1.2.ringleadermanaged").unwrap();

        assert_eq!(marker.branch(), "Feature/v1.2");
    }

    #[test]
    fn a_key_of_another_variable_is_no_marker() {
        assert!(BranchMarker::from_key("branch.fix-auth.remote").is_none());
    }

    #[test]
    fn a_key_of_another_section_is_no_marker() {
        assert!(BranchMarker::from_key("remote.fix-auth.ringleadermanaged").is_none());
    }
}

pub struct BranchMarker<'a> {
    branch: &'a str,
}

impl<'a> BranchMarker<'a> {
    pub const VARIABLE: &'static str = "ringleaderManaged";
    pub const VALUE: &'static str = "true";

    pub fn new(branch: &'a str) -> Self {
        Self { branch }
    }

    pub fn key(&self) -> String {
        format!("branch.{}.{}", self.branch, Self::VARIABLE)
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
}

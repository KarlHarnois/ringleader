use crate::domain::Branch;

pub struct BranchRef<'a> {
    branch: &'a Branch,
}

impl<'a> BranchRef<'a> {
    pub const LOCAL_PREFIX: &'static str = "refs/heads/";
    pub const REMOTE_TRACKING_PREFIX: &'static str = "refs/remotes/";

    pub fn new(branch: &'a Branch) -> Self {
        Self { branch }
    }

    pub fn parse(full_name: &str) -> Option<Branch> {
        let local = full_name
            .strip_prefix(Self::LOCAL_PREFIX)
            .map(|name| Branch::Local(name.to_string()));
        local.or_else(|| {
            full_name
                .strip_prefix(Self::REMOTE_TRACKING_PREFIX)
                .map(|name| Branch::RemoteTracking(name.to_string()))
        })
    }

    pub fn full_name(&self) -> String {
        match self.branch {
            Branch::Local(name) => format!("{}{name}", Self::LOCAL_PREFIX),
            Branch::RemoteTracking(name) => format!("{}{name}", Self::REMOTE_TRACKING_PREFIX),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_branch_is_spelled_under_refs_heads() {
        let main = Branch::Local("main".to_string());

        assert_eq!(BranchRef::new(&main).full_name(), "refs/heads/main");
    }

    #[test]
    fn a_remote_tracking_branch_is_spelled_under_refs_remotes() {
        let origin_main = Branch::RemoteTracking("origin/main".to_string());

        assert_eq!(
            BranchRef::new(&origin_main).full_name(),
            "refs/remotes/origin/main"
        );
    }

    #[test]
    fn a_full_name_parses_back_to_its_branch() {
        assert_eq!(
            BranchRef::parse("refs/remotes/origin/main"),
            Some(Branch::RemoteTracking("origin/main".to_string()))
        );
    }

    #[test]
    fn a_name_outside_the_branch_namespaces_is_no_branch() {
        assert_eq!(BranchRef::parse("HEAD"), None);
    }
}

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

    pub fn name(&self) -> &str {
        match self {
            Branch::Local(name) | Branch::RemoteTracking(name) => name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_origin_copy_of_a_branch_is_tracked_under_origin() {
        assert_eq!(
            Branch::origin_copy_of("main"),
            Branch::RemoteTracking("origin/main".to_string())
        );
    }

    #[test]
    fn a_branch_is_named_as_users_type_it() {
        let origin_copy = Branch::origin_copy_of("develop");

        assert_eq!(origin_copy.name(), "origin/develop");
    }
}

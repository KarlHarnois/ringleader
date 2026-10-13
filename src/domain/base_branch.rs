use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BaseBranch(String);

impl BaseBranch {
    const ORIGIN_PREFIX: &'static str = "origin/";

    pub fn new(name: &str) -> Option<Self> {
        let is_blank = name.trim().is_empty();
        (!is_blank).then(|| Self(name.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn local_ref(&self) -> String {
        format!("refs/heads/{}", self.0)
    }

    pub fn remote_tracking_ref(&self) -> String {
        format!("refs/remotes/{}", self.0)
    }

    pub fn candidate_refs(&self) -> [String; 2] {
        [self.local_ref(), self.remote_tracking_ref()]
    }

    pub fn origin_suggestion(&self) -> String {
        format!("{}{}", Self::ORIGIN_PREFIX, self.0)
    }

    pub fn origin_ref(&self) -> String {
        format!("refs/remotes/{}", self.origin_suggestion())
    }

    pub fn on_origin(&self) -> Option<Self> {
        Self::new(self.0.strip_prefix(Self::ORIGIN_PREFIX).unwrap_or(&self.0))
    }
}

impl fmt::Display for BaseBranch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_name_is_no_base_branch() {
        assert_eq!(BaseBranch::new("  "), None);
    }

    #[test]
    fn names_a_local_branch_ref_rather_than_the_bare_name() {
        let base = BaseBranch::new("main").unwrap();

        assert_eq!(base.local_ref(), "refs/heads/main");
    }

    #[test]
    fn names_a_remote_tracking_ref() {
        let base = BaseBranch::new("origin/main").unwrap();

        assert_eq!(base.remote_tracking_ref(), "refs/remotes/origin/main");
    }

    #[test]
    fn tries_the_local_branch_before_the_remote_tracking_one() {
        let base = BaseBranch::new("develop").unwrap();

        assert_eq!(
            base.candidate_refs(),
            ["refs/heads/develop", "refs/remotes/develop"]
        );
    }

    #[test]
    fn suggests_the_origin_copy() {
        let base = BaseBranch::new("develop").unwrap();

        assert_eq!(base.origin_suggestion(), "origin/develop");
    }

    #[test]
    fn names_the_ref_of_the_origin_copy() {
        let base = BaseBranch::new("develop").unwrap();

        assert_eq!(base.origin_ref(), "refs/remotes/origin/develop");
    }

    #[test]
    fn a_local_name_keeps_its_name_on_origin() {
        let base = BaseBranch::new("develop").unwrap();

        assert_eq!(base.on_origin(), BaseBranch::new("develop"));
    }

    #[test]
    fn an_origin_tracking_name_drops_the_remote_on_origin() {
        let base = BaseBranch::new("origin/main").unwrap();

        assert_eq!(base.on_origin(), BaseBranch::new("main"));
    }

    #[test]
    fn the_bare_origin_prefix_names_no_branch_on_origin() {
        let base = BaseBranch::new("origin/").unwrap();

        assert_eq!(base.on_origin(), None);
    }
}

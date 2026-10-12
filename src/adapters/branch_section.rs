pub struct BranchSection<'a> {
    branch: &'a str,
}

impl<'a> BranchSection<'a> {
    const REGEX_SPECIAL: &'static str = "\\.^$*+?()[]{}|";

    pub fn new(branch: &'a str) -> Self {
        Self { branch }
    }

    pub fn name(&self) -> String {
        format!("branch.{}", self.branch)
    }

    pub fn upstream_remote_key(&self) -> String {
        format!("{}.remote", self.name())
    }

    pub fn upstream_merge_key(&self) -> String {
        format!("{}.merge", self.name())
    }

    pub fn key_pattern(&self) -> String {
        let escaped: String = self
            .name()
            .chars()
            .flat_map(|character| {
                let escape = Self::REGEX_SPECIAL.contains(character).then_some('\\');
                escape.into_iter().chain([character])
            })
            .collect();
        format!("^{escaped}\\.")
    }
}

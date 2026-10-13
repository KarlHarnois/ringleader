use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BaseBranch(String);

impl BaseBranch {
    pub fn new(name: &str) -> Option<Self> {
        let is_blank = name.trim().is_empty();
        (!is_blank).then(|| Self(name.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
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
}

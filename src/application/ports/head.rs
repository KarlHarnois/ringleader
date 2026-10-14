use crate::domain::Branch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    Unborn,
    Detached,
    On(Branch),
}

impl Head {
    pub fn is_unborn(&self) -> bool {
        matches!(self, Head::Unborn)
    }

    pub fn branch(&self) -> Option<&Branch> {
        match self {
            Head::On(branch) => Some(branch),
            Head::Unborn | Head::Detached => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_head_on_a_branch_names_it() {
        let main = Branch::Local("main".to_string());

        assert_eq!(Head::On(main.clone()).branch(), Some(&main));
    }

    #[test]
    fn a_detached_head_names_no_branch() {
        assert_eq!(Head::Detached.branch(), None);
    }
}

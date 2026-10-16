#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Checkout {
    #[default]
    NewWorktree,
    Main,
}

impl Checkout {
    pub fn toggled(self) -> Self {
        match self {
            Self::NewWorktree => Self::Main,
            Self::Main => Self::NewWorktree,
        }
    }

    pub fn creates_a_worktree(self) -> bool {
        self == Self::NewWorktree
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::NewWorktree => "new worktree",
            Self::Main => "main checkout",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_checkout_is_a_new_worktree() {
        assert_eq!(Checkout::default(), Checkout::NewWorktree);
    }

    #[test]
    fn toggling_a_new_worktree_gives_the_main_checkout() {
        assert_eq!(Checkout::NewWorktree.toggled(), Checkout::Main);
    }

    #[test]
    fn toggling_the_main_checkout_gives_a_new_worktree() {
        assert_eq!(Checkout::Main.toggled(), Checkout::NewWorktree);
    }

    #[test]
    fn only_a_new_worktree_creates_a_worktree() {
        assert!(Checkout::NewWorktree.creates_a_worktree());
        assert!(!Checkout::Main.creates_a_worktree());
    }
}

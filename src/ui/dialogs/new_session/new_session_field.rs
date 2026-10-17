#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewSessionField {
    Name,
    Repo,
    Prompt,
}

impl NewSessionField {
    const ORDER: [NewSessionField; 3] = [Self::Name, Self::Repo, Self::Prompt];

    pub fn next(self) -> Self {
        Self::ORDER[(self.position() + 1) % Self::ORDER.len()]
    }

    pub fn is_last(self) -> bool {
        Self::ORDER.last() == Some(&self)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Repo => "Repo",
            Self::Prompt => "Prompt",
        }
    }

    pub fn hints(self) -> &'static str {
        match self {
            Self::Name => "Enter next field · Ctrl-S create · Shift-Tab mode · Esc cancel",
            Self::Repo => "Type to find a repo · Enter next field · Ctrl-S create · Esc cancel",
            Self::Prompt => {
                "Enter create · Alt-P agent · Shift-Tab mode · Ctrl-T checkout · Esc cancel"
            }
        }
    }

    fn position(self) -> usize {
        Self::ORDER
            .iter()
            .position(|field| *field == self)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_wraps_from_the_last_field_to_the_first() {
        assert_eq!(NewSessionField::Prompt.next(), NewSessionField::Name);
    }

    #[test]
    fn the_repo_is_followed_by_the_prompt() {
        assert_eq!(NewSessionField::Repo.next(), NewSessionField::Prompt);
    }
}

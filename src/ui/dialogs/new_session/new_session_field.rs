#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewSessionField {
    Repo,
    Name,
    Prompt,
}

impl NewSessionField {
    const ORDER: [NewSessionField; 3] = [Self::Repo, Self::Name, Self::Prompt];

    pub fn next(self) -> Self {
        Self::ORDER[(self.position() + 1) % Self::ORDER.len()]
    }

    pub fn is_last(self) -> bool {
        Self::ORDER.last() == Some(&self)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Repo => "Repo",
            Self::Name => "Name",
            Self::Prompt => "Prompt",
        }
    }

    pub fn hints(self) -> &'static str {
        match self {
            Self::Repo => "Type to find a repo · Enter next field · Ctrl-S create · Esc cancel",
            Self::Name => "Enter next field · Ctrl-S create · Esc cancel",
            Self::Prompt => "Enter create · Esc cancel",
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
        assert_eq!(NewSessionField::Prompt.next(), NewSessionField::Repo);
    }

    #[test]
    fn the_repo_is_followed_by_the_name() {
        assert_eq!(NewSessionField::Repo.next(), NewSessionField::Name);
    }

    #[test]
    fn the_name_is_followed_by_the_prompt() {
        assert_eq!(NewSessionField::Name.next(), NewSessionField::Prompt);
    }
}

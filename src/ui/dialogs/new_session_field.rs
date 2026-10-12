#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewSessionField {
    Name,
    Repo,
    Agent,
    Prompt,
}

impl NewSessionField {
    const ORDER: [NewSessionField; 4] = [Self::Name, Self::Repo, Self::Agent, Self::Prompt];

    pub fn next(self) -> Self {
        Self::ORDER[(self.position() + 1) % Self::ORDER.len()]
    }

    pub fn previous(self) -> Self {
        let count = Self::ORDER.len();
        Self::ORDER[(self.position() + count - 1) % count]
    }

    pub fn is_last(self) -> bool {
        Self::ORDER.last() == Some(&self)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Repo => "Repo",
            Self::Agent => "Agent",
            Self::Prompt => "Prompt",
        }
    }

    pub fn hints(self) -> &'static str {
        match self {
            Self::Name => "Enter next field · Ctrl-S create · Esc cancel",
            Self::Repo => "Type to find a repo · Enter next field · Ctrl-S create · Esc cancel",
            Self::Agent => "↑ ↓ j k choose · Enter next field · Ctrl-S create · Esc cancel",
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
        assert_eq!(NewSessionField::Prompt.next(), NewSessionField::Name);
    }

    #[test]
    fn previous_wraps_from_the_first_field_to_the_last() {
        assert_eq!(NewSessionField::Name.previous(), NewSessionField::Prompt);
    }
}

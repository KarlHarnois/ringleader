use std::fmt;

pub struct SessionCount(usize);

impl SessionCount {
    pub fn new(count: usize) -> Self {
        Self(count)
    }
}

impl fmt::Display for SessionCount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            1 => write!(formatter, "1 session"),
            count => write!(formatter, "{count} sessions"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_session_is_singular() {
        assert_eq!(SessionCount::new(1).to_string(), "1 session");
    }

    #[test]
    fn other_counts_are_plural() {
        assert_eq!(SessionCount::new(0).to_string(), "0 sessions");
        assert_eq!(SessionCount::new(3).to_string(), "3 sessions");
    }
}

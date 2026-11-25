#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteHostEntry {
    name: String,
}

impl RemoteHostEntry {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn name_refusal(&self) -> Option<&'static str> {
        if self.name.is_empty() {
            Some("is empty")
        } else if self.name.chars().any(char::is_whitespace) {
            Some("holds whitespace")
        } else if self.name.starts_with('-') {
            Some("starts with -")
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_name_is_refused() {
        assert_eq!(RemoteHostEntry::new("").name_refusal(), Some("is empty"));
    }

    #[test]
    fn a_name_holding_whitespace_is_refused() {
        assert_eq!(
            RemoteHostEntry::new("two words").name_refusal(),
            Some("holds whitespace")
        );
    }

    #[test]
    fn a_name_starting_with_a_dash_is_refused() {
        assert_eq!(
            RemoteHostEntry::new("-oProxyCommand").name_refusal(),
            Some("starts with -")
        );
    }

    #[test]
    fn destinations_that_ssh_takes_are_accepted() {
        for name in ["example-host", "user@example-host", "192.0.2.10"] {
            assert_eq!(RemoteHostEntry::new(name).name_refusal(), None, "{name}");
        }
    }
}

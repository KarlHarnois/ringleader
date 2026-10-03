use std::ffi::{OsStr, OsString};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessEnvironment {
    variables: Vec<(OsString, OsString)>,
    terminal: Option<OsString>,
}

impl ProcessEnvironment {
    pub fn new(variables: Vec<(OsString, OsString)>, terminal: Option<OsString>) -> Self {
        Self {
            variables,
            terminal,
        }
    }

    pub fn get(&self, name: &str) -> Option<&OsStr> {
        self.variables
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value.as_os_str())
    }

    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn without(&self, names: &[String]) -> Self {
        Self {
            variables: self
                .variables
                .iter()
                .filter(|(name, _)| !names.iter().any(|removed| name == removed.as_str()))
                .cloned()
                .collect(),
            terminal: self.terminal.clone(),
        }
    }

    pub fn variables(&self) -> &[(OsString, OsString)] {
        &self.variables
    }

    pub fn terminal(&self) -> Option<&OsStr> {
        self.terminal.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvironmentFixture;

    impl EnvironmentFixture {
        fn with(pairs: &[(&str, &str)]) -> ProcessEnvironment {
            let variables = pairs
                .iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value)))
                .collect();
            ProcessEnvironment::new(variables, None)
        }
    }

    #[test]
    fn without_removes_the_named_variables() {
        let environment = EnvironmentFixture::with(&[("API_KEY", "secret"), ("PATH", "/bin")]);

        let filtered = environment.without(&["API_KEY".to_string()]);

        assert!(!filtered.contains("API_KEY"));
        assert_eq!(filtered.get("PATH"), Some(OsStr::new("/bin")));
    }
}

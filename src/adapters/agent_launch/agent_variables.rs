use std::ffi::OsString;

use crate::adapters::host::process_environment::ProcessEnvironment;

pub struct AgentVariables {
    inherited: Vec<(OsString, OsString)>,
    explicit: Vec<(OsString, OsString)>,
    missing: Vec<String>,
}

impl AgentVariables {
    pub fn new(
        environment: &ProcessEnvironment,
        env_names: &[String],
        env_assignments: &[(String, String)],
    ) -> Self {
        let (named, missing) = Self::named(environment, env_names);
        let baseline = environment.baseline().variables().to_vec();
        let shell = environment.shell_variable().into_iter().collect();
        let kept = Self::overridden(shell, named.clone());
        Self {
            inherited: [baseline, named].concat(),
            explicit: Self::overridden(kept, Self::assigned(env_assignments)),
            missing,
        }
    }

    pub fn inherited(&self) -> &[(OsString, OsString)] {
        &self.inherited
    }

    pub fn explicit(&self) -> &[(OsString, OsString)] {
        &self.explicit
    }

    pub fn missing(&self) -> &[String] {
        &self.missing
    }

    pub fn value(&self, name: &str) -> Option<&OsString> {
        self.explicit
            .iter()
            .rev()
            .chain(self.inherited.iter().rev())
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value)
    }

    pub fn combined(&self) -> Vec<(OsString, OsString)> {
        Self::overridden(self.inherited.clone(), self.explicit.clone())
    }

    fn named(
        environment: &ProcessEnvironment,
        env_names: &[String],
    ) -> (Vec<(OsString, OsString)>, Vec<String>) {
        let mut named = Vec::new();
        let mut missing = Vec::new();
        for name in env_names {
            match environment.get(name) {
                Some(value) => named.push((OsString::from(name), value.to_os_string())),
                None => missing.push(name.clone()),
            }
        }
        (named, missing)
    }

    fn assigned(env_assignments: &[(String, String)]) -> Vec<(OsString, OsString)> {
        env_assignments
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect()
    }

    fn overridden(
        variables: Vec<(OsString, OsString)>,
        overrides: Vec<(OsString, OsString)>,
    ) -> Vec<(OsString, OsString)> {
        let is_overridden =
            |name: &OsString| overrides.iter().any(|(overriding, _)| overriding == name);
        let mut combined: Vec<(OsString, OsString)> = variables
            .into_iter()
            .filter(|(name, _)| !is_overridden(name))
            .collect();
        combined.extend(overrides);
        combined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct VariablesFixture;

    impl VariablesFixture {
        fn environment() -> ProcessEnvironment {
            ProcessEnvironment::new(
                vec![
                    (OsString::from("HOME"), OsString::from("/home/example")),
                    (OsString::from("EXAMPLE_TOKEN"), OsString::from("inherited")),
                ],
                None,
            )
        }

        fn names(names: &[&str]) -> Vec<String> {
            names.iter().map(|name| name.to_string()).collect()
        }
    }

    #[test]
    fn a_named_variable_has_its_inherited_value() {
        let names = VariablesFixture::names(&["EXAMPLE_TOKEN"]);
        let variables = AgentVariables::new(&VariablesFixture::environment(), &names, &[]);

        assert_eq!(
            variables.value("EXAMPLE_TOKEN"),
            Some(&OsString::from("inherited"))
        );
    }

    #[test]
    fn an_assigned_value_wins_over_the_inherited_one() {
        let names = VariablesFixture::names(&["EXAMPLE_TOKEN"]);
        let assigned = [("EXAMPLE_TOKEN".to_string(), "assigned".to_string())];
        let variables = AgentVariables::new(&VariablesFixture::environment(), &names, &assigned);

        assert_eq!(
            variables.value("EXAMPLE_TOKEN"),
            Some(&OsString::from("assigned"))
        );
    }

    #[test]
    fn a_variable_the_agent_does_not_name_has_no_value() {
        let variables = AgentVariables::new(&VariablesFixture::environment(), &[], &[]);

        assert_eq!(variables.value("EXAMPLE_TOKEN"), None);
    }

    #[test]
    fn a_baseline_variable_has_its_value() {
        let variables = AgentVariables::new(&VariablesFixture::environment(), &[], &[]);

        assert_eq!(
            variables.value("HOME"),
            Some(&OsString::from("/home/example"))
        );
    }
}

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

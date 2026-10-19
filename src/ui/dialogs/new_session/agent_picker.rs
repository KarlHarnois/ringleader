use std::path::{Path, PathBuf};

use crate::domain::Agent;

pub struct AgentPicker {
    agents: Vec<Agent>,
    selected: usize,
    config_path: PathBuf,
}

impl AgentPicker {
    pub fn new(agents: &[Agent], config_path: &Path) -> Self {
        Self {
            agents: agents.to_vec(),
            selected: 0,
            config_path: config_path.to_path_buf(),
        }
    }

    pub fn selected(&self) -> Result<&Agent, String> {
        self.agents
            .get(self.selected)
            .ok_or_else(|| format!("no agents configured in {}", self.config_path.display()))
    }

    pub fn cycle(&mut self) {
        self.selected = (self.selected + 1) % self.agents.len().max(1);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    struct PickerFixture;

    impl PickerFixture {
        const CONFIG_PATH: &'static str = "/config/ringleader/config.toml";

        fn with_agents(names: &[&str]) -> AgentPicker {
            let agents: Vec<Agent> = names.iter().map(|name| Agent::new(name, "sh")).collect();
            AgentPicker::new(&agents, Path::new(Self::CONFIG_PATH))
        }

        fn selected_name(picker: &AgentPicker) -> String {
            picker.selected().unwrap().name().to_string()
        }
    }

    #[test]
    fn cycling_picks_the_next_agent() {
        let mut picker = PickerFixture::with_agents(&["claude", "opencode"]);

        picker.cycle();

        assert_eq!(PickerFixture::selected_name(&picker), "opencode");
    }

    #[test]
    fn cycling_past_the_last_agent_wraps_to_the_first() {
        let mut picker = PickerFixture::with_agents(&["claude", "opencode"]);

        picker.cycle();
        picker.cycle();

        assert_eq!(PickerFixture::selected_name(&picker), "claude");
    }

    #[test]
    fn cycling_with_no_agents_keeps_the_config_path_error() {
        let mut picker = PickerFixture::with_agents(&[]);

        picker.cycle();

        assert_eq!(
            picker.selected().err(),
            Some(format!(
                "no agents configured in {}",
                PickerFixture::CONFIG_PATH
            ))
        );
    }
}

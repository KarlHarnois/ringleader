use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::Line;

use crate::domain::Agent;
use crate::ui::theme::Theme;

pub struct AgentPicker {
    agents: Vec<Agent>,
    selected: usize,
    config_path: PathBuf,
}

impl AgentPicker {
    const NO_PROMPT_MARK: &'static str = " · no prompt";

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

    pub fn height(&self, expanded: bool) -> usize {
        if expanded {
            self.agents.len().max(1)
        } else {
            1
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        let last = self.agents.len().saturating_sub(1);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.selected = (self.selected + 1).min(last),
            _ => return false,
        }
        true
    }

    pub fn lines(&self, expanded: bool, theme: &Theme) -> Vec<Line<'static>> {
        let selected = match self.selected() {
            Ok(agent) => agent,
            Err(problem) => return vec![Line::styled(problem, theme.error())],
        };
        if !expanded {
            return vec![Line::from(Self::text(selected))];
        }
        self.agents
            .iter()
            .enumerate()
            .map(|(index, agent)| {
                let style = if index == self.selected {
                    theme.selected()
                } else {
                    theme.base()
                };
                Line::styled(Self::text(agent), style)
            })
            .collect()
    }

    fn text(agent: &Agent) -> String {
        let mark = if agent.accepts_prompt() {
            ""
        } else {
            Self::NO_PROMPT_MARK
        };
        format!("{}{mark}", agent.name())
    }
}

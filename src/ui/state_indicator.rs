use ratatui::style::Style;

use super::theme::Theme;
use crate::domain::SessionState;

pub struct StateIndicator {
    state: SessionState,
    tick: u64,
}

impl StateIndicator {
    const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

    pub fn new(state: SessionState, tick: u64) -> Self {
        Self { state, tick }
    }

    pub fn glyph(&self) -> char {
        match self.state {
            SessionState::Working => {
                let frame = self.tick % Self::SPINNER.len() as u64;
                Self::SPINNER[usize::try_from(frame).unwrap_or(0)]
            }
            SessionState::Waiting => '●',
            SessionState::Exited => '✗',
            SessionState::Stopped => '○',
            SessionState::Unmanaged => '?',
        }
    }

    pub fn style(&self, theme: &Theme) -> Style {
        theme.state(self.state)
    }
}

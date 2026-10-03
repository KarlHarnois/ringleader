use ratatui::style::{Color, Modifier, Style};

use crate::domain::SessionState;

#[derive(Debug, Clone, Copy, Default)]
pub struct Theme;

impl Theme {
    pub fn base(&self) -> Style {
        Style::default().fg(Color::Reset).bg(Color::Reset)
    }

    pub fn selected(&self) -> Style {
        self.base().add_modifier(Modifier::REVERSED)
    }

    pub fn dim(&self) -> Style {
        self.base().add_modifier(Modifier::DIM)
    }

    pub fn emphasis(&self) -> Style {
        self.base().add_modifier(Modifier::BOLD)
    }

    pub fn error(&self) -> Style {
        self.base().fg(Color::Red)
    }

    pub fn warning(&self) -> Style {
        self.base().fg(Color::Yellow)
    }

    pub fn insertions(&self) -> Style {
        self.base().fg(Color::Green)
    }

    pub fn deletions(&self) -> Style {
        self.base().fg(Color::Red)
    }

    pub fn state(&self, state: SessionState) -> Style {
        match state {
            SessionState::Working => self.base(),
            SessionState::Waiting => self.base().fg(Color::Green),
            SessionState::Exited => self.base().fg(Color::Red),
            SessionState::Stopped => self.dim(),
            SessionState::Unmanaged => self.warning(),
        }
    }
}

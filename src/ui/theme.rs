use ratatui::style::{Color, Modifier, Style};

use crate::domain::{PullRequestState, SessionState};

#[derive(Debug, Clone, Copy, Default)]
pub struct Theme;

impl Theme {
    pub fn base(&self) -> Style {
        Style::default().fg(Color::Reset).bg(Color::Reset)
    }

    pub fn selected(&self) -> Style {
        self.base().add_modifier(Modifier::REVERSED)
    }

    pub fn on_selection(&self, mut style: Style) -> Style {
        style.add_modifier.remove(Modifier::DIM);
        style
    }

    pub fn selected_text(&self) -> Style {
        self.base().add_modifier(Modifier::UNDERLINED)
    }

    pub fn dim(&self) -> Style {
        self.base().add_modifier(Modifier::DIM)
    }

    pub fn emphasis(&self) -> Style {
        self.base().add_modifier(Modifier::BOLD)
    }

    pub fn matched(&self) -> Style {
        self.emphasis().add_modifier(Modifier::UNDERLINED)
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

    pub fn pull_request(&self, state: PullRequestState) -> Style {
        match state {
            PullRequestState::Open => self.base().fg(Color::Green),
            PullRequestState::Draft => self.warning(),
            PullRequestState::Merged => self.base().fg(Color::Magenta),
            PullRequestState::Closed => self.base().fg(Color::Red),
        }
    }
}

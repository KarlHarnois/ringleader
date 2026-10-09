use ratatui::text::Line;

use crate::application::{Pending, SessionService};
use crate::domain::Repo;
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub enum TypedRepoCheck {
    Checking {
        typed: String,
        pending: Pending<Repo>,
    },
    Refused(String),
}

impl TypedRepoCheck {
    pub fn start(service: &SessionService, typed: &str) -> Self {
        Self::Checking {
            typed: typed.to_string(),
            pending: service.repo_at(typed),
        }
    }

    pub fn poll(&mut self) -> Option<Repo> {
        let Self::Checking { pending, .. } = self else {
            return None;
        };
        match pending.poll()? {
            Ok(repo) => Some(repo),
            Err(error) => {
                *self = Self::Refused(error.to_string());
                None
            }
        }
    }

    pub fn lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        match self {
            Self::Checking { typed, .. } => {
                let checking = format!("Checking {typed}…");
                TextFit::new(&checking).styled_lines(width, theme.dim())
            }
            Self::Refused(error) => TextFit::new(error).styled_lines(width, theme.error()),
        }
    }
}

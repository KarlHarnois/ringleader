use ratatui::text::Line;

use crate::application::{Pending, SessionService};
use crate::domain::Repo;
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct TypedRepoCheck {
    typed: String,
    pending: Option<Pending<Repo>>,
    error: Option<String>,
}

impl TypedRepoCheck {
    pub fn start(service: &SessionService, typed: &str) -> Self {
        Self {
            typed: typed.to_string(),
            pending: Some(service.repo_at(typed)),
            error: None,
        }
    }

    pub fn poll(&mut self) -> Option<Repo> {
        let result = self.pending.as_ref()?.poll()?;
        self.pending = None;
        match result {
            Ok(repo) => Some(repo),
            Err(error) => {
                self.error = Some(error.to_string());
                None
            }
        }
    }

    pub fn lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        if self.pending.is_some() {
            let checking = format!("Checking {}…", self.typed);
            return TextFit::new(&checking).styled_lines(width, theme.dim());
        }
        match &self.error {
            Some(error) => TextFit::new(error).styled_lines(width, theme.error()),
            None => Vec::new(),
        }
    }
}

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::Line;

use super::text_field::{FieldKind, TextField};
use crate::application::{Pending, SessionService};
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct ScopeInput {
    field: TextField,
    checking: Option<Pending<PathBuf>>,
    error: Option<String>,
}

impl Default for ScopeInput {
    fn default() -> Self {
        Self {
            field: TextField::new(FieldKind::Path),
            checking: None,
            error: None,
        }
    }
}

impl ScopeInput {
    const LABEL: &'static str = "Scope path: ";
    const CURSOR: char = '█';

    pub fn handle_key(&mut self, key: &KeyEvent, service: &SessionService) -> bool {
        if key.code == KeyCode::Enter {
            self.error = None;
            self.checking = Some(service.read_typed_path(self.field.value()));
            return true;
        }
        self.field.edit(key)
    }

    pub fn paste(&mut self, text: &str) {
        self.error = self.field.paste(text).err();
    }

    pub fn checked_path(&mut self) -> Option<PathBuf> {
        let result = self.checking.as_ref()?.poll()?;
        self.checking = None;
        match result {
            Ok(path) => Some(path),
            Err(error) => {
                self.error = Some(error.to_string());
                None
            }
        }
    }

    pub fn lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let room = width.saturating_sub(Self::LABEL.len());
        let shown = self.field.visible(room, Some(Self::CURSOR));
        let mut lines = vec![Line::from(format!("{}{shown}", Self::LABEL))];
        if self.checking.is_some() {
            lines.push(Line::styled("Checking the path…", theme.dim()));
        }
        if let Some(error) = &self.error {
            lines.extend(TextFit::new(error).styled_lines(width, theme.error()));
        }
        lines
    }
}

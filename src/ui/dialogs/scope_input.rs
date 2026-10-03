use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::Line;

use super::text_field::{FieldKind, TextField};
use crate::application::{Pending, SessionService};
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
    pub const HEIGHT: usize = 2;

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
        let mut lines = vec![Line::from(format!(
            "Path: {}",
            self.field.visible(width, Some('█'))
        ))];
        if self.checking.is_some() {
            lines.push(Line::styled("Checking the path…", theme.dim()));
        }
        if let Some(error) = &self.error {
            lines.push(Line::styled(error.clone(), theme.error()));
        }
        lines
    }
}

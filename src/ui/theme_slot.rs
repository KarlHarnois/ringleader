use super::theme::{ColorMode, Theme};
use crate::application::SessionService;

#[derive(Debug, Clone, Copy, Default)]
pub struct ThemeSlot {
    color_mode: ColorMode,
    theme: Theme,
}

impl ThemeSlot {
    pub fn new(color_mode: ColorMode) -> Self {
        Self {
            color_mode,
            theme: Theme::default(),
        }
    }

    pub fn reload(&mut self, service: &mut SessionService) {
        service.load_theme();
        self.theme = self.color_mode.theme(service.theme());
    }

    pub fn theme(&self) -> Theme {
        self.theme
    }
}

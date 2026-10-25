use crossterm::event::{KeyCode, KeyEvent};

use super::dialog::DialogStep;

pub struct ConfirmationKeys;

impl ConfirmationKeys {
    pub const HINTS: &'static str = "Enter delete · Esc cancel";
    pub const ARMED_HINTS: &'static str =
        "This loses work. Press Enter again to delete · Esc cancel";

    pub fn step(key: &KeyEvent, confirm: impl FnOnce() -> DialogStep) -> DialogStep {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n' | 'q') => DialogStep::Close,
            KeyCode::Enter | KeyCode::Char('y') => confirm(),
            _ => DialogStep::Stay,
        }
    }
}

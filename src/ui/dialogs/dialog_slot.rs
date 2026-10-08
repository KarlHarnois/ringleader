use crossterm::event::Event;
use ratatui::Frame;

use super::dialog::{Dialog, DialogStep};
use crate::application::SessionService;
use crate::ui::theme::Theme;

#[derive(Default)]
pub struct DialogSlot {
    dialog: Option<Dialog>,
}

impl DialogSlot {
    pub fn is_open(&self) -> bool {
        self.dialog.is_some()
    }

    pub fn open(&mut self, dialog: Dialog) {
        self.dialog = Some(dialog);
    }

    pub fn close(&mut self) {
        self.dialog = None;
    }

    pub fn poll(&mut self) {
        if let Some(dialog) = &mut self.dialog {
            dialog.poll();
        }
    }

    pub fn handle_event(&mut self, event: &Event, service: &SessionService) -> DialogStep {
        let Some(dialog) = &mut self.dialog else {
            return DialogStep::Stay;
        };
        let step = dialog.handle_event(event, service);
        if step == DialogStep::Close {
            self.close();
        }
        step
    }

    pub fn fail(&mut self, error: String) -> Result<(), String> {
        match self
            .dialog
            .as_mut()
            .filter(|dialog| dialog.shows_failures())
        {
            Some(dialog) => {
                dialog.fail(error);
                Ok(())
            }
            None => {
                self.close();
                Err(error)
            }
        }
    }

    pub fn render(&self, frame: &mut Frame, theme: &Theme) {
        if let Some(dialog) = &self.dialog {
            dialog.render(frame, theme);
        }
    }
}

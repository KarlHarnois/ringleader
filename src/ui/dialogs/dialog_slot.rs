use std::time::Instant;

use crossterm::event::Event;
use ratatui::Frame;

use super::dialog::{Dialog, DialogStep};
use crate::application::{ServiceError, SessionService};
use crate::domain::SessionSummary;
use crate::ui::keys::back_key::BackKey;
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

    pub fn poll(
        &mut self,
        now: Instant,
        service: &SessionService,
        sessions_to_sample: Option<&[SessionSummary]>,
    ) -> Result<(), ServiceError> {
        match &mut self.dialog {
            Some(dialog) => dialog.poll(now, service, sessions_to_sample),
            None => Ok(()),
        }
    }

    pub fn handle_event(&mut self, event: &Event, service: &SessionService) -> DialogStep {
        let Some(dialog) = &mut self.dialog else {
            return DialogStep::Stay;
        };
        let step = match event {
            Event::Key(key) if BackKey::matches(key) => DialogStep::Close,
            _ => dialog.handle_event(event, service),
        };
        if matches!(step, DialogStep::Close | DialogStep::CloseWith(_)) {
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

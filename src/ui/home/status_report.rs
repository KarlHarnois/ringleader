use std::fmt::Display;

use super::status_bar::StatusMessage;

#[derive(Debug, Default)]
pub struct StatusReport {
    message: Option<StatusMessage>,
}

impl StatusReport {
    pub fn message(&self) -> Option<&StatusMessage> {
        self.message.as_ref()
    }

    pub fn show(&mut self, message: Option<StatusMessage>) {
        self.message = message;
    }

    pub fn info(&mut self, text: String) {
        self.message = Some(StatusMessage::Info(text));
    }

    pub fn error(&mut self, text: String) {
        self.message = Some(StatusMessage::Error(text));
    }

    pub fn failure(&mut self, outcome: Result<(), impl Display>) {
        if let Err(error) = outcome {
            self.error(error.to_string());
        }
    }

    pub fn outcome(&mut self, outcome: Result<(), impl Display>, success: String) {
        self.message = Some(match outcome {
            Ok(()) => StatusMessage::Info(success),
            Err(error) => StatusMessage::Error(error.to_string()),
        });
    }
}

use super::{CliExit, CliOutput};
use crate::application::ports::{Interrupts, Lock};
use crate::application::{Pending, ServiceError, SessionService};

pub struct CliSession {
    output: CliOutput,
    signals: Box<dyn Interrupts>,
}

impl CliSession {
    const SIGNAL_EXIT_BASE: u8 = 128;

    pub fn new(output: CliOutput, signals: Box<dyn Interrupts>) -> Self {
        Self { output, signals }
    }

    pub fn output(&mut self) -> &mut CliOutput {
        &mut self.output
    }

    pub fn stop_if_signaled(&self) -> Result<(), CommandStop> {
        match self.signals.raised() {
            Some(signal) => Err(CommandStop::Interrupted(signal)),
            None => Ok(()),
        }
    }

    pub fn wait<T: Send + 'static>(&self, pending: &Pending<T>) -> Result<T, CommandStop> {
        match pending.wait_unless(|| self.is_signaled()) {
            Some(result) => result.map_err(CommandStop::Failed),
            None => Err(CommandStop::Interrupted(
                self.signals.raised().unwrap_or_default(),
            )),
        }
    }

    pub fn wait_for_startup_input(&mut self, service: &SessionService, lock: &mut dyn Lock) {
        lock.release();
        if let Some(note) = service.startup_wait_note()
            && !self.is_signaled()
        {
            self.output.note(note);
        }
        service.wait_startup_input(|| self.is_signaled());
    }

    pub fn finish(&mut self, result: Result<u8, CommandStop>) -> CliExit {
        let code = match result {
            Ok(code) => code,
            Err(CommandStop::Failed(error)) => {
                self.output.error(&error.to_string());
                CliExit::error_code(&error)
            }
            Err(CommandStop::Interrupted(signal)) => {
                return CliExit::Interrupted {
                    signal,
                    code: Self::SIGNAL_EXIT_BASE.saturating_add(u8::try_from(signal).unwrap_or(0)),
                };
            }
        };
        match self.signals.raised() {
            Some(signal) => {
                self.output
                    .error("interrupted, the command finished before stopping");
                CliExit::Interrupted { signal, code }
            }
            None => CliExit::Code(code),
        }
    }

    fn is_signaled(&self) -> bool {
        self.signals.raised().is_some()
    }
}

pub enum CommandStop {
    Failed(ServiceError),
    Interrupted(i32),
}

impl From<ServiceError> for CommandStop {
    fn from(error: ServiceError) -> Self {
        CommandStop::Failed(error)
    }
}

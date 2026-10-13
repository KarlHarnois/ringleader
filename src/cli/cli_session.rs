use super::CliOutput;
use crate::adapters::signal_flags::SignalFlags;
use crate::application::{Pending, ServiceError};

pub struct CliSession {
    output: CliOutput,
    signals: SignalFlags,
}

impl CliSession {
    const SIGNAL_EXIT_BASE: u8 = 128;

    pub fn new(output: CliOutput, signals: SignalFlags) -> Self {
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

    pub fn is_signaled(&self) -> bool {
        self.signals.raised().is_some()
    }

    pub fn wait<T: Send + 'static>(&self, pending: &Pending<T>) -> Result<T, CommandStop> {
        match pending.wait_unless(|| self.signals.raised().is_some()) {
            Some(result) => result.map_err(CommandStop::Failed),
            None => Err(CommandStop::Interrupted(
                self.signals.raised().unwrap_or_default(),
            )),
        }
    }

    pub fn finish(&mut self, result: Result<u8, CommandStop>) -> CliExit {
        let code = match result {
            Ok(code) => code,
            Err(CommandStop::Failed(error)) => {
                self.output.error(&error.to_string());
                error.exit_code()
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliExit {
    Code(u8),
    Interrupted { signal: i32, code: u8 },
}

impl CliExit {
    pub fn code(&self) -> u8 {
        match self {
            CliExit::Code(code) | CliExit::Interrupted { code, .. } => *code,
        }
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

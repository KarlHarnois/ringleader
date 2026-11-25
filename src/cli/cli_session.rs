use super::prompt_argument::{PromptArgument, PromptArgumentError};
use super::prompt_text::PromptText;
use super::{CliExit, CliInput, CliOutput, EnvelopeError};
use crate::application::ports::{Interrupts, PromptFiles};
use crate::application::{FailureKind, Pending, ServiceError, SessionService};

pub struct CliSession {
    output: CliOutput,
    input: CliInput,
    prompt_files: Box<dyn PromptFiles>,
    signals: Box<dyn Interrupts>,
}

impl CliSession {
    const SIGNAL_EXIT_BASE: u8 = 128;

    pub fn new(
        output: CliOutput,
        input: CliInput,
        prompt_files: Box<dyn PromptFiles>,
        signals: Box<dyn Interrupts>,
    ) -> Self {
        Self {
            output,
            input,
            prompt_files,
            signals,
        }
    }

    pub fn output(&mut self) -> &mut CliOutput {
        &mut self.output
    }

    pub fn read_prompt(
        &mut self,
        argument: &PromptArgument,
    ) -> Result<Option<PromptText>, PromptArgumentError> {
        argument.read(self.prompt_files.as_ref(), &mut self.input)
    }

    pub fn stop_if_signaled(&self) -> Result<(), CommandStop> {
        match self.signals.raised() {
            Some(signal) => Err(CommandStop::Interrupted(signal)),
            None => Ok(()),
        }
    }

    pub fn wait<T: Send + 'static>(&self, pending: &Pending<T>) -> Result<T, CommandStop> {
        match pending.wait_unless(|| self.is_signaled()) {
            Some(result) => result.map_err(CommandStop::from),
            None => Err(CommandStop::Interrupted(
                self.signals.raised().unwrap_or_default(),
            )),
        }
    }

    pub fn wait_for_startup_input(&mut self, service: &SessionService) {
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
            Err(CommandStop::Failed(failure)) => {
                let kind = failure.kind();
                if self.is_signaled() {
                    self.output.print_interrupted_failure(failure.message());
                } else {
                    self.output.print_failure(kind, failure.message());
                }
                CliExit::exit_code(kind)
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

    #[cfg(test)]
    pub fn finish_failure(&mut self, error: ServiceError) -> CliExit {
        self.finish(Err(error.into()))
    }

    pub fn is_signaled(&self) -> bool {
        self.signals.raised().is_some()
    }
}

pub enum CommandStop {
    Failed(CommandFailure),
    Interrupted(i32),
}

pub struct CommandFailure {
    kind: FailureKind,
    message: String,
}

impl CommandFailure {
    fn kind(&self) -> FailureKind {
        self.kind
    }

    fn message(&self) -> &str {
        &self.message
    }
}

impl From<ServiceError> for CommandStop {
    fn from(error: ServiceError) -> Self {
        CommandStop::Failed(CommandFailure {
            kind: error.failure_kind(),
            message: error.to_string(),
        })
    }
}

impl From<EnvelopeError> for CommandStop {
    fn from(error: EnvelopeError) -> Self {
        CommandStop::Failed(CommandFailure {
            kind: error.failure_kind(),
            message: error.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::OutputMode;
    use crate::cli::envelope::Envelope;
    use crate::test_support::{
        CapturedWriter, FakeCliInput, FakeInterrupts, FakePromptFiles, UnserializableValue,
    };

    #[test]
    fn finishing_with_an_envelope_error_prints_the_failed_envelope_and_exits_one() {
        let stdout = CapturedWriter::default();
        let stderr = CapturedWriter::default();
        let output = CliOutput::new(
            Box::new(stdout.clone()),
            Box::new(stderr.clone()),
            OutputMode::Json,
        );
        let mut session = CliSession::new(
            output,
            CliInput::new(Box::new(FakeCliInput::default())),
            Box::new(FakePromptFiles::default()),
            Box::new(FakeInterrupts::default()),
        );
        let error = Envelope::of(&UnserializableValue).text().unwrap_err();

        let exit = session.finish(Err(CommandStop::from(error)));

        let json: serde_json::Value = serde_json::from_str(stdout.text().trim()).unwrap();
        assert_eq!(json["error"]["code"], "failed");
        assert_eq!(
            json["error"]["message"],
            "cannot serialize the JSON envelope"
        );
        assert_eq!(stderr.text(), "");
        assert_eq!(exit, CliExit::Code(CliExit::FAILURE));
    }
}

use std::io::{self, Write};

use super::envelope::Envelope;
use super::reply::Reply;
use super::{CliExit, EnvelopeError};
use crate::application::FailureKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Text,
    Json,
}

pub struct CliOutput {
    stdout: Box<dyn Write>,
    stderr: Box<dyn Write>,
    stdout_closed: bool,
    mode: OutputMode,
}

impl CliOutput {
    pub(super) const PROGRAM: &'static str = "ringleader";

    pub fn new(stdout: Box<dyn Write>, stderr: Box<dyn Write>, mode: OutputMode) -> Self {
        Self {
            stdout,
            stderr,
            stdout_closed: false,
            mode,
        }
    }

    pub fn standard(mode: OutputMode) -> Self {
        Self::new(Box::new(io::stdout()), Box::new(io::stderr()), mode)
    }

    pub fn reply(&mut self, reply: &impl Reply) -> CliExit {
        match self.mode {
            OutputMode::Text => {
                for line in reply.lines() {
                    self.line(&line);
                }
                CliExit::Code(0)
            }
            OutputMode::Json => self.envelope(Envelope::of(&reply.result()).text()),
        }
    }

    fn envelope(&mut self, text: Result<String, EnvelopeError>) -> CliExit {
        match text {
            Ok(text) => {
                self.line(&text);
                CliExit::Code(0)
            }
            Err(error) => {
                self.error(&error.to_string());
                CliExit::Code(CliExit::FAILURE)
            }
        }
    }

    pub fn print_failure(&mut self, kind: FailureKind, message: &str) {
        match self.mode {
            OutputMode::Text => self.error(message),
            OutputMode::Json => match Envelope::error(kind, message).text() {
                Ok(text) => self.line(&text),
                Err(error) => self.error(&error.to_string()),
            },
        }
    }

    pub fn print_interrupted_failure(&mut self, message: &str) {
        match self.mode {
            OutputMode::Text => self.error(message),
            OutputMode::Json => {}
        }
    }

    pub fn line(&mut self, text: &str) {
        if self.stdout_closed {
            return;
        }
        if let Err(error) = writeln!(self.stdout, "{text}")
            && error.kind() == io::ErrorKind::BrokenPipe
        {
            self.stdout_closed = true;
        }
    }

    pub fn error(&mut self, text: &str) {
        let _ = writeln!(self.stderr, "{}: {text}", Self::PROGRAM);
    }

    pub fn note(&mut self, text: &str) {
        let _ = writeln!(self.stderr, "{text}");
    }

    #[cfg(test)]
    pub fn is_closed(&self) -> bool {
        self.stdout_closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{CapturedWriter, UnserializableValue};

    #[test]
    fn an_envelope_that_renders_writes_the_line_and_exits_zero() {
        let stdout = CapturedWriter::default();
        let stderr = CapturedWriter::default();
        let mut output = CliOutput::new(
            Box::new(stdout.clone()),
            Box::new(stderr.clone()),
            OutputMode::Text,
        );

        let exit = output.envelope(Ok("line".to_string()));

        assert_eq!(exit, CliExit::Code(0));
        assert_eq!(stdout.text(), "line\n");
        assert_eq!(stderr.text(), "");
    }

    #[test]
    fn an_envelope_that_fails_writes_the_message_and_exits_one() {
        let stdout = CapturedWriter::default();
        let stderr = CapturedWriter::default();
        let mut output = CliOutput::new(
            Box::new(stdout.clone()),
            Box::new(stderr.clone()),
            OutputMode::Text,
        );
        let error = Envelope::of(&UnserializableValue).text().unwrap_err();

        let exit = output.envelope(Err(error));

        assert_eq!(exit, CliExit::Code(CliExit::FAILURE));
        assert_eq!(stdout.text(), "");
        assert_eq!(
            stderr.text(),
            "ringleader: cannot serialize the JSON envelope\n"
        );
    }

    #[test]
    fn a_broken_pipe_stops_further_lines_without_panicking() {
        let stderr = CapturedWriter::default();
        let mut output = CliOutput::new(
            Box::new(CapturedWriter::broken_pipe()),
            Box::new(stderr),
            OutputMode::Text,
        );

        output.line("first");
        output.line("second");

        assert!(output.is_closed());
    }

    #[test]
    fn errors_carry_the_program_name() {
        let stderr = CapturedWriter::default();
        let mut output = CliOutput::new(
            Box::new(CapturedWriter::default()),
            Box::new(stderr.clone()),
            OutputMode::Text,
        );

        output.error("no session named x");

        assert_eq!(stderr.text(), "ringleader: no session named x\n");
    }
}

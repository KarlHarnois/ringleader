use std::io::{self, Write};

use super::{CliExit, EnvelopeError};

pub struct CliOutput {
    stdout: Box<dyn Write>,
    stderr: Box<dyn Write>,
    stdout_closed: bool,
}

impl CliOutput {
    pub(super) const PROGRAM: &'static str = "ringleader";

    pub fn new(stdout: Box<dyn Write>, stderr: Box<dyn Write>) -> Self {
        Self {
            stdout,
            stderr,
            stdout_closed: false,
        }
    }

    pub fn standard() -> Self {
        Self::new(Box::new(io::stdout()), Box::new(io::stderr()))
    }

    pub fn envelope(&mut self, text: Result<String, EnvelopeError>) -> CliExit {
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
    use serde::Serialize;

    use super::*;
    use crate::cli::envelope::Envelope;
    use crate::test_support::CapturedWriter;

    struct Unserializable;

    impl Serialize for Unserializable {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(serde::ser::Error::custom("not serializable"))
        }
    }

    #[test]
    fn an_envelope_that_renders_writes_the_line_and_exits_zero() {
        let stdout = CapturedWriter::default();
        let stderr = CapturedWriter::default();
        let mut output = CliOutput::new(Box::new(stdout.clone()), Box::new(stderr.clone()));

        let exit = output.envelope(Ok("line".to_string()));

        assert_eq!(exit, CliExit::Code(0));
        assert_eq!(stdout.text(), "line\n");
        assert_eq!(stderr.text(), "");
    }

    #[test]
    fn an_envelope_that_fails_writes_the_message_and_exits_one() {
        let stdout = CapturedWriter::default();
        let stderr = CapturedWriter::default();
        let mut output = CliOutput::new(Box::new(stdout.clone()), Box::new(stderr.clone()));
        let error = Envelope::of(&Unserializable).text().unwrap_err();

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
        let mut output = CliOutput::new(Box::new(CapturedWriter::broken_pipe()), Box::new(stderr));

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
        );

        output.error("no session named x");

        assert_eq!(stderr.text(), "ringleader: no session named x\n");
    }
}

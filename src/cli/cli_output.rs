use std::io::{self, Write};

use super::EnvelopeError;
use super::envelope::Envelope;
use super::reply::Reply;
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

    pub fn reply(&mut self, reply: &impl Reply) -> Result<(), EnvelopeError> {
        match self.mode {
            OutputMode::Text => {
                for line in reply.lines() {
                    self.line(&line);
                }
                for notice in reply.notices() {
                    self.error(&notice);
                }
                Ok(())
            }
            OutputMode::Json => {
                let text = Envelope::of(&reply.result()).text()?;
                self.line(&text);
                Ok(())
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
    use serde::Serialize;

    use super::*;
    use crate::test_support::{CapturedWriter, UnserializableValue};

    struct OutputFixture {
        output: CliOutput,
        stdout: CapturedWriter,
        stderr: CapturedWriter,
    }

    impl OutputFixture {
        fn of(mode: OutputMode) -> Self {
            Self::with_stdout(mode, CapturedWriter::default())
        }

        fn with_broken_stdout() -> Self {
            Self::with_stdout(OutputMode::Text, CapturedWriter::broken_pipe())
        }

        fn with_stdout(mode: OutputMode, stdout: CapturedWriter) -> Self {
            let stderr = CapturedWriter::default();
            let output = CliOutput::new(Box::new(stdout.clone()), Box::new(stderr.clone()), mode);
            Self {
                output,
                stdout,
                stderr,
            }
        }

        fn stdout_text(&self) -> String {
            self.stdout.text()
        }

        fn stderr_text(&self) -> String {
            self.stderr.text()
        }
    }

    struct LinesReply;

    #[derive(Serialize)]
    struct LinesResult {
        value: &'static str,
    }

    impl Reply for LinesReply {
        fn lines(&self) -> Vec<String> {
            vec!["first".to_string(), "second".to_string()]
        }

        fn result(&self) -> impl Serialize {
            LinesResult { value: "ok" }
        }
    }

    struct UnserializableReply;

    impl Reply for UnserializableReply {
        fn lines(&self) -> Vec<String> {
            Vec::new()
        }

        fn result(&self) -> impl Serialize {
            UnserializableValue
        }
    }

    #[test]
    fn a_text_reply_prints_each_line_on_standard_output() {
        let mut fixture = OutputFixture::of(OutputMode::Text);

        fixture.output.reply(&LinesReply).unwrap();

        assert_eq!(fixture.stdout_text(), "first\nsecond\n");
        assert_eq!(fixture.stderr_text(), "");
    }

    #[test]
    fn a_json_reply_prints_one_line_on_standard_output() {
        let mut fixture = OutputFixture::of(OutputMode::Json);

        fixture.output.reply(&LinesReply).unwrap();

        let json: serde_json::Value = serde_json::from_str(fixture.stdout_text().trim()).unwrap();
        assert_eq!(json["result"]["value"], "ok");
        assert_eq!(fixture.stdout_text().lines().count(), 1);
        assert_eq!(fixture.stderr_text(), "");
    }

    #[test]
    fn a_json_reply_that_cannot_be_serialized_prints_nothing_and_returns_the_error() {
        let mut fixture = OutputFixture::of(OutputMode::Json);

        let error = fixture.output.reply(&UnserializableReply).unwrap_err();

        assert_eq!(error.to_string(), "cannot serialize the JSON envelope");
        assert_eq!(fixture.stdout_text(), "");
        assert_eq!(fixture.stderr_text(), "");
    }

    #[test]
    fn a_broken_pipe_stops_further_lines_without_panicking() {
        let mut fixture = OutputFixture::with_broken_stdout();

        fixture.output.line("first");
        fixture.output.line("second");

        assert!(fixture.output.is_closed());
    }

    #[test]
    fn errors_carry_the_program_name() {
        let mut fixture = OutputFixture::of(OutputMode::Text);

        fixture.output.error("no session named x");

        assert_eq!(fixture.stderr_text(), "ringleader: no session named x\n");
    }
}

use std::io::{self, Write};

pub struct CliOutput {
    stdout: Box<dyn Write>,
    stderr: Box<dyn Write>,
    stdout_closed: bool,
}

impl CliOutput {
    const PROGRAM: &'static str = "ringleader";

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
    use crate::test_support::CapturedWriter;

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

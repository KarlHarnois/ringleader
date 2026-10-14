use std::cell::RefCell;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use clap::Parser;

use super::{FakeInterrupts, FakeLock, ServiceFixture};
use crate::cli::{Cli, CliExit, CliOutput, CliSession};

pub struct CliHarness {
    fixture: ServiceFixture,
    stdout: CapturedWriter,
    stderr: CapturedWriter,
    signals: FakeInterrupts,
    lock: RefCell<FakeLock>,
}

impl CliHarness {
    pub fn new(fixture: ServiceFixture) -> Self {
        Self {
            fixture,
            stdout: CapturedWriter::default(),
            stderr: CapturedWriter::default(),
            signals: FakeInterrupts::default(),
            lock: RefCell::new(FakeLock::free()),
        }
    }

    pub fn with_lock(self, lock: FakeLock) -> Self {
        Self {
            lock: RefCell::new(lock),
            ..self
        }
    }

    pub fn lock_acquisitions(&self) -> Vec<&'static str> {
        self.lock.borrow().acquisitions().to_vec()
    }

    pub fn with_stdout(self, stdout: CapturedWriter) -> Self {
        Self { stdout, ..self }
    }

    pub fn with_stderr(self, stderr: CapturedWriter) -> Self {
        Self { stderr, ..self }
    }

    pub fn run(&self, arguments: &[&str]) -> CliExit {
        let cli =
            Cli::try_parse_from(std::iter::once("ringleader").chain(arguments.iter().copied()))
                .expect("the arguments parse");
        let output = CliOutput::new(Box::new(self.stdout.clone()), Box::new(self.stderr.clone()));
        let mut session = CliSession::new(output, Box::new(self.signals.clone()));
        if let Err(exit) = cli.take_write_lock(&mut *self.lock.borrow_mut(), &mut session) {
            return exit;
        }
        cli.execute(
            &mut self.fixture.unloaded_service(),
            &mut session,
            &mut *self.lock.borrow_mut(),
        )
    }

    pub fn fixture(&self) -> &ServiceFixture {
        &self.fixture
    }

    pub fn signals(&self) -> &FakeInterrupts {
        &self.signals
    }

    pub fn stdout(&self) -> String {
        self.stdout.text()
    }

    pub fn stderr(&self) -> String {
        self.stderr.text()
    }
}

#[derive(Clone, Default)]
pub struct CapturedWriter {
    buffer: Arc<Mutex<Vec<u8>>>,
    broken: bool,
}

impl CapturedWriter {
    pub fn broken_pipe() -> Self {
        Self {
            broken: true,
            ..Self::default()
        }
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.buffer.lock().unwrap()).into_owned()
    }
}

impl Write for CapturedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.broken {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        }
        self.buffer.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

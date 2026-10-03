use std::io::Read;
use std::process::{Child, ExitStatus};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct CommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl CommandOutput {
    pub fn succeeded(&self) -> bool {
        self.status.success()
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.status.code()
    }

    pub fn status_label(&self) -> String {
        self.status.to_string()
    }

    pub fn stdout(&self) -> &[u8] {
        &self.stdout
    }

    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).trim().to_string()
    }
}

pub struct PendingOutput {
    stdout: Option<Receiver<Vec<u8>>>,
    stderr: Option<Receiver<Vec<u8>>>,
}

impl PendingOutput {
    pub fn collect(child: &mut Child) -> Self {
        Self {
            stdout: child.stdout.take().map(Self::read_in_background),
            stderr: child.stderr.take().map(Self::read_in_background),
        }
    }

    fn read_in_background(mut pipe: impl Read + Send + 'static) -> Receiver<Vec<u8>> {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            let _ = sender.send(bytes);
        });
        receiver
    }

    pub fn finish(self, status: ExitStatus, reader_grace: Duration) -> CommandOutput {
        let reader_deadline = Instant::now() + reader_grace;
        CommandOutput {
            status,
            stdout: Self::received(self.stdout, reader_deadline),
            stderr: Self::received(self.stderr, reader_deadline),
        }
    }

    fn received(receiver: Option<Receiver<Vec<u8>>>, deadline: Instant) -> Vec<u8> {
        let Some(receiver) = receiver else {
            return Vec::new();
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        receiver.recv_timeout(remaining).unwrap_or_default()
    }
}

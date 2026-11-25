use std::io::{self, Read};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct FakeCliInput {
    bytes: Arc<Vec<u8>>,
    position: usize,
    log: Option<Arc<Mutex<Vec<String>>>>,
}

impl FakeCliInput {
    pub const READ_TO_END: &'static str = "read to end";

    pub fn holding(text: &str) -> Self {
        Self::containing(text.as_bytes())
    }

    pub fn containing(bytes: &[u8]) -> Self {
        Self {
            bytes: Arc::new(bytes.to_vec()),
            position: 0,
            log: None,
        }
    }

    pub fn recording(self, log: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            log: Some(log),
            ..self
        }
    }

    fn record_end(&self) {
        if let Some(log) = &self.log {
            log.lock().unwrap().push(Self::READ_TO_END.to_string());
        }
    }
}

impl Read for FakeCliInput {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let remaining = &self.bytes[self.position..];
        if remaining.is_empty() {
            self.record_end();
            return Ok(0);
        }
        let count = remaining.len().min(buffer.len());
        buffer[..count].copy_from_slice(&remaining[..count]);
        self.position += count;
        Ok(count)
    }
}

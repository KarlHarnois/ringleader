use std::sync::{Mutex, Once, OnceLock};

use log::{LevelFilter, Log, Metadata, Record};

pub struct LogCapture {
    messages: Mutex<Vec<String>>,
}

impl LogCapture {
    pub fn installed() -> &'static Self {
        static CAPTURE: OnceLock<LogCapture> = OnceLock::new();
        static INSTALL: Once = Once::new();
        let capture = CAPTURE.get_or_init(|| Self {
            messages: Mutex::default(),
        });
        INSTALL.call_once(|| {
            log::set_logger(capture).expect("the test binary installs no other logger");
            log::set_max_level(LevelFilter::Warn);
        });
        capture
    }

    pub fn count_containing(&self, text: &str) -> usize {
        self.messages
            .lock()
            .unwrap()
            .iter()
            .filter(|message| message.contains(text))
            .count()
    }
}

impl Log for LogCapture {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        self.messages
            .lock()
            .unwrap()
            .push(record.args().to_string());
    }

    fn flush(&self) {}
}

use std::io;
use std::sync::{Arc, Mutex};

use crate::ui::terminal_guard::ScreenMode;

#[derive(Clone, Default)]
pub struct RecordingScreenMode {
    switches: Arc<Mutex<Vec<&'static str>>>,
}

impl RecordingScreenMode {
    pub fn switches(&self) -> Vec<&'static str> {
        self.switches.lock().unwrap().clone()
    }

    fn record(&self, switch: &'static str) {
        self.switches.lock().unwrap().push(switch);
    }
}

impl ScreenMode for RecordingScreenMode {
    fn enter(&mut self) -> io::Result<()> {
        self.record("enter");
        Ok(())
    }

    fn leave(&mut self) -> io::Result<()> {
        self.record("leave");
        Ok(())
    }
}

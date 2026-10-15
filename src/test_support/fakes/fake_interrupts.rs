use std::sync::{Arc, Mutex};

use crate::application::ports::Interrupts;

#[derive(Clone, Default)]
pub struct FakeInterrupts {
    raised: Arc<Mutex<Option<i32>>>,
}

impl FakeInterrupts {
    pub fn raise(&self, signal: i32) {
        *self.raised.lock().unwrap() = Some(signal);
    }
}

impl Interrupts for FakeInterrupts {
    fn raised(&self) -> Option<i32> {
        *self.raised.lock().unwrap()
    }
}

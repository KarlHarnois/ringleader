use std::sync::Mutex;

use crate::application::ports::{BindingError, ReturnBinding};

#[derive(Default)]
pub struct FakeReturnBinding {
    calls: Mutex<Vec<&'static str>>,
    install_error: Mutex<Option<BindingError>>,
    restore_error: Mutex<Option<BindingError>>,
    repair_error: Mutex<Option<BindingError>>,
}

impl FakeReturnBinding {
    pub fn failing_install(self, error: BindingError) -> Self {
        *self.install_error.lock().unwrap() = Some(error);
        self
    }

    pub fn failing_restore(self, error: BindingError) -> Self {
        *self.restore_error.lock().unwrap() = Some(error);
        self
    }

    pub fn failing_repair(self, error: BindingError) -> Self {
        *self.repair_error.lock().unwrap() = Some(error);
        self
    }

    pub fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().unwrap().clone()
    }

    fn record(&self, call: &'static str) {
        self.calls.lock().unwrap().push(call);
    }
}

impl ReturnBinding for FakeReturnBinding {
    fn install(&self) -> Result<(), BindingError> {
        self.record("install");
        match self.install_error.lock().unwrap().clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn restore(&self) -> Result<(), BindingError> {
        self.record("restore");
        self.restore_error
            .lock()
            .unwrap()
            .clone()
            .map_or(Ok(()), Err)
    }

    fn repair_stale(&self) -> Result<(), BindingError> {
        self.record("repair_stale");
        self.repair_error
            .lock()
            .unwrap()
            .clone()
            .map_or(Ok(()), Err)
    }
}

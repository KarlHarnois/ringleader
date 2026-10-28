use std::sync::Mutex;

use crate::application::ports::{ThemeError, ThemeSource};
use crate::domain::ThemeOverrides;

pub struct FakeThemeSource {
    loaded: Mutex<Result<ThemeOverrides, ThemeError>>,
}

impl Default for FakeThemeSource {
    fn default() -> Self {
        Self {
            loaded: Mutex::new(Ok(ThemeOverrides::default())),
        }
    }
}

impl FakeThemeSource {
    pub fn replace(&self, overrides: ThemeOverrides) {
        *self.loaded.lock().unwrap() = Ok(overrides);
    }

    pub fn break_with(&self, error: ThemeError) {
        *self.loaded.lock().unwrap() = Err(error);
    }
}

impl ThemeSource for FakeThemeSource {
    fn load(&self) -> Result<ThemeOverrides, ThemeError> {
        self.loaded.lock().unwrap().clone()
    }
}

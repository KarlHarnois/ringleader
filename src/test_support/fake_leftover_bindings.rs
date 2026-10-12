use std::path::PathBuf;

use crate::application::ports::{LeftoverBinding, LeftoverBindings, LeftoverBindingsError};

#[derive(Default)]
pub struct FakeLeftoverBindings {
    bindings: Vec<LeftoverBinding>,
    error: Option<LeftoverBindingsError>,
}

impl FakeLeftoverBindings {
    pub const PATH: &'static str = "/state/ringleader/bindings.json";

    pub fn with(bindings: Vec<LeftoverBinding>) -> Self {
        Self {
            bindings,
            error: None,
        }
    }

    pub fn failing(error: LeftoverBindingsError) -> Self {
        Self {
            bindings: Vec::new(),
            error: Some(error),
        }
    }
}

impl LeftoverBindings for FakeLeftoverBindings {
    fn path(&self) -> PathBuf {
        PathBuf::from(Self::PATH)
    }

    fn read(&self) -> Result<Vec<LeftoverBinding>, LeftoverBindingsError> {
        match &self.error {
            Some(error) => Err(error.clone()),
            None => Ok(self.bindings.clone()),
        }
    }
}

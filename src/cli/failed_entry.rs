use serde::Serialize;

use super::Remark;
use super::envelope::ErrorBody;
use crate::application::ServiceError;

#[derive(Serialize)]
pub struct FailedEntry {
    name: String,
    error: ErrorBody,
}

impl FailedEntry {
    pub fn of(name: &str, error: &ServiceError) -> Self {
        Self {
            name: name.to_string(),
            error: ErrorBody::of(error),
        }
    }

    pub fn remark(&self) -> Remark {
        Remark::Problem(format!("{}: {}", self.name, self.error.message()))
    }
}

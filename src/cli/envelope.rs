use serde::Serialize;
use thiserror::Error;

use crate::application::FailureKind;
use crate::domain::{BuildVersion, ContractVersion};

pub struct Envelope<'a, T: Serialize> {
    outcome: Outcome<'a, T>,
}

impl<'a, T: Serialize> Envelope<'a, T> {
    pub fn of(result: &'a T) -> Self {
        Self {
            outcome: Outcome::Result(result),
        }
    }

    pub fn text(&self) -> Result<String, EnvelopeError> {
        let (result, error) = match &self.outcome {
            Outcome::Result(result) => (Some(*result), None),
            Outcome::Error { kind, message } => (
                None,
                Some(ErrorBody {
                    code: kind.code(),
                    message,
                }),
            ),
        };
        serde_json::to_string(&Body {
            schema_version: ContractVersion::CURRENT.number(),
            build_version: BuildVersion::CURRENT.text(),
            result,
            error,
        })
        .map_err(EnvelopeError)
    }
}

impl<'a> Envelope<'a, ()> {
    pub fn error(kind: FailureKind, message: &'a str) -> Self {
        Self {
            outcome: Outcome::Error { kind, message },
        }
    }
}

enum Outcome<'a, T> {
    Result(&'a T),
    Error { kind: FailureKind, message: &'a str },
}

#[derive(Debug, Error)]
#[error("cannot serialize the JSON envelope")]
pub struct EnvelopeError(#[source] serde_json::Error);

#[derive(Serialize)]
struct Body<'a, T: Serialize> {
    schema_version: u32,
    build_version: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<&'a T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ErrorBody<'a>>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'static str,
    message: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::UnserializableValue;

    #[test]
    fn a_value_that_cannot_be_serialized_fails_with_the_envelope_error() {
        let error = Envelope::of(&UnserializableValue).text().unwrap_err();

        assert_eq!(error.to_string(), "cannot serialize the JSON envelope");
    }
}

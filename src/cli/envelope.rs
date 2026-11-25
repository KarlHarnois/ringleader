use serde::Serialize;
use thiserror::Error;

use crate::domain::{BuildVersion, ContractVersion};

pub struct Envelope<'a, T: Serialize> {
    result: &'a T,
}

impl<'a, T: Serialize> Envelope<'a, T> {
    pub fn of(result: &'a T) -> Self {
        Self { result }
    }

    pub fn text(&self) -> Result<String, EnvelopeError> {
        serde_json::to_string(&Body {
            schema_version: ContractVersion::CURRENT.number(),
            build_version: BuildVersion::CURRENT.text(),
            result: self.result,
        })
        .map_err(EnvelopeError)
    }
}

#[derive(Debug, Error)]
#[error("cannot serialize the JSON envelope")]
pub struct EnvelopeError(#[source] serde_json::Error);

#[derive(Serialize)]
struct Body<'a, T: Serialize> {
    schema_version: u32,
    build_version: &'a str,
    result: &'a T,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Unserializable;

    impl Serialize for Unserializable {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(serde::ser::Error::custom("not serializable"))
        }
    }

    #[test]
    fn a_value_that_cannot_be_serialized_fails_with_the_envelope_error() {
        let error = Envelope::of(&Unserializable).text().unwrap_err();

        assert_eq!(error.to_string(), "cannot serialize the JSON envelope");
    }
}

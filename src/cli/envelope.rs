use serde::Serialize;

use crate::domain::{BuildVersion, ContractVersion};

pub struct Envelope {
    result: serde_json::Value,
}

impl Envelope {
    pub fn of(result: &impl Serialize) -> Self {
        Self {
            result: serde_json::to_value(result).unwrap_or(serde_json::Value::Null),
        }
    }

    pub fn text(&self) -> String {
        serde_json::to_string(&Body {
            schema_version: ContractVersion::CURRENT.number(),
            build_version: BuildVersion::CURRENT.text(),
            result: &self.result,
        })
        .unwrap_or_default()
    }
}

#[derive(Serialize)]
struct Body<'a> {
    schema_version: u32,
    build_version: &'a str,
    result: &'a serde_json::Value,
}

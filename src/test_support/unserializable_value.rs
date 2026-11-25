use serde::Serialize;

pub struct UnserializableValue;

impl Serialize for UnserializableValue {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        Err(serde::ser::Error::custom("not serializable"))
    }
}

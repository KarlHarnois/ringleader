use serde::Serialize;

pub trait Reply {
    fn lines(&self) -> Vec<String>;
    fn result(&self) -> impl Serialize;
}

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remark {
    Note(String),
    Problem(String),
}

pub trait Reply {
    fn lines(&self) -> Vec<String>;

    fn remarks(&self) -> Vec<Remark> {
        Vec::new()
    }

    fn result(&self) -> impl Serialize;
}

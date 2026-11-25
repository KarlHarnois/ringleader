use serde::Serialize;

pub trait Reply {
    fn lines(&self) -> Vec<String>;

    fn notices(&self) -> Vec<String> {
        Vec::new()
    }

    fn result(&self) -> impl Serialize;
}

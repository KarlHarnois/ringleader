use std::io::{self, Read};

pub struct CliInput {
    reader: Box<dyn Read>,
}

impl CliInput {
    pub fn new(reader: Box<dyn Read>) -> Self {
        Self { reader }
    }

    pub fn standard() -> Self {
        Self::new(Box::new(io::stdin()))
    }

    pub fn read_text(&mut self) -> io::Result<String> {
        let mut text = String::new();
        self.reader.read_to_string(&mut text).map(|_| text)
    }
}

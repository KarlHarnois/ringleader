use serde::Serialize;

use super::Reply;

pub struct SendReply {
    name: String,
}

impl SendReply {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

impl Reply for SendReply {
    fn lines(&self) -> Vec<String> {
        vec![format!("sent the prompt to {}", self.name)]
    }

    fn result(&self) -> impl Serialize {
        SendResult {
            name: self.name.clone(),
            sent: true,
        }
    }
}

#[derive(Serialize)]
struct SendResult {
    name: String,
    sent: bool,
}

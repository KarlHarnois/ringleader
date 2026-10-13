use super::tmux_output::TmuxOutput;
use super::tmux_server_config::TmuxServerConfig;
use crate::domain::ReturnKey;

pub struct RunningConfig {
    hash: String,
    return_key: Option<ReturnKey>,
}

impl RunningConfig {
    const LABEL: &'static str = "ringleader-config";

    pub fn format() -> String {
        format!(
            "{} #{{{}}} #{{{}}}",
            Self::LABEL,
            TmuxServerConfig::HASH_OPTION,
            TmuxServerConfig::RETURN_KEY_OPTION
        )
    }

    pub fn read(output: &TmuxOutput) -> Option<Self> {
        let values = output.first(Self::LABEL)?;
        let (hash, return_key) = values.split_once(' ').unwrap_or((values, ""));
        Some(Self {
            hash: hash.to_string(),
            return_key: ReturnKey::parse(return_key),
        })
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }

    pub fn return_key(&self) -> Option<&ReturnKey> {
        self.return_key.as_ref()
    }
}

use super::tmux_command::TmuxCommand;
use super::tmux_error::TmuxError;
use super::tmux_output::TmuxOutput;
use super::tmux_sequence::TmuxSequence;
use super::tmux_server_config::TmuxServerConfig;
use crate::application::ports::MultiplexerError;
use crate::domain::{HotKey, KeyBindings};

pub struct KeySwitch {
    restored: DisplacedBindings,
    displaced: DisplacedBindings,
}

impl KeySwitch {
    pub fn read(
        tmux: &TmuxCommand,
        previous: Option<&KeyBindings>,
        current: &KeyBindings,
    ) -> Result<Self, MultiplexerError> {
        let Some(previous) = previous else {
            return Ok(Self::unchanged());
        };
        let dropped: Vec<&HotKey> = previous.all().filter(|key| !current.binds(key)).collect();
        let added: Vec<&HotKey> = current.all().filter(|key| !previous.binds(key)).collect();
        Ok(Self {
            restored: DisplacedBindings::saved_for(tmux, &dropped)?,
            displaced: DisplacedBindings::replaced_by(tmux, &added)?,
        })
    }

    pub fn saving(&self) -> TmuxSequence {
        self.displaced.saving()
    }

    pub fn restoring_lines(&self) -> String {
        self.restored.replay()
    }

    fn unchanged() -> Self {
        Self {
            restored: DisplacedBindings::default(),
            displaced: DisplacedBindings::default(),
        }
    }
}

#[derive(Default)]
struct DisplacedBindings {
    slots: Vec<DisplacedBinding>,
}

struct DisplacedBinding {
    option: String,
    line: Option<String>,
}

impl DisplacedBindings {
    const OPTION_PREFIX: &'static str = "@ringleader_displaced_";

    fn saved_for(tmux: &TmuxCommand, keys: &[&HotKey]) -> Result<Self, MultiplexerError> {
        Self::per_slot(keys, |option, _, _| {
            let query = TmuxSequence::new().command(["show-options", "-gqv", option]);
            let output = tmux.query(&query)?;
            match output.succeeded() {
                true => Ok(Self::first_line(&output)),
                false => Err(TmuxError::failed("show-options", &output).into()),
            }
        })
    }

    fn replaced_by(tmux: &TmuxCommand, keys: &[&HotKey]) -> Result<Self, MultiplexerError> {
        Self::per_slot(keys, |_, table, key| {
            let query = TmuxSequence::new().command(["list-keys", "-T", table, &key.to_string()]);
            let output = tmux.query(&query)?;
            Ok(Self::first_line(&output).filter(|_| output.succeeded()))
        })
    }

    fn saving(&self) -> TmuxSequence {
        self.slots
            .iter()
            .fold(TmuxSequence::new(), |sequence, slot| {
                let line = slot.line.as_deref().unwrap_or_default();
                sequence.command(["set-option", "-g", &slot.option, line])
            })
    }

    fn replay(&self) -> String {
        self.slots
            .iter()
            .filter_map(|slot| slot.line.as_deref())
            .map(|line| format!("{line}\n"))
            .collect()
    }

    fn per_slot(
        keys: &[&HotKey],
        read: impl Fn(&str, &str, &HotKey) -> Result<Option<String>, MultiplexerError>,
    ) -> Result<Self, MultiplexerError> {
        let mut slots = Vec::new();
        for key in keys {
            for table in TmuxServerConfig::COPY_TABLES {
                let option = Self::option(table, key);
                let line = read(&option, table, key)?;
                slots.push(DisplacedBinding { option, line });
            }
        }
        Ok(Self { slots })
    }

    fn option(table: &str, key: &HotKey) -> String {
        let key_bytes: String = key
            .to_string()
            .bytes()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!(
            "{}{}_{key_bytes}",
            Self::OPTION_PREFIX,
            table.replace('-', "_")
        )
    }

    fn first_line(output: &TmuxOutput) -> Option<String> {
        output
            .stdout()
            .lines()
            .next()
            .filter(|line| !line.is_empty())
            .map(str::to_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TmuxTestServer;

    #[test]
    fn a_server_that_cannot_report_the_saved_bindings_fails_the_switch() {
        let server = TmuxTestServer::new();
        let tmux = server.command();
        let previous = KeyBindings::default().with_return_key(HotKey::parse("M-x").unwrap());

        let switch = KeySwitch::read(&tmux, Some(&previous), &KeyBindings::default());

        assert!(switch.is_err());
    }
}

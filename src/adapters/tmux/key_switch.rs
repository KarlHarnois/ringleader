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
    const TABLE_FLAG: &'static str = "-T";

    fn saved_for(tmux: &TmuxCommand, keys: &[&HotKey]) -> Result<Self, MultiplexerError> {
        Self::per_slot(keys, |option| {
            let query = TmuxSequence::new().command(["show-options", "-gqv", option]);
            let output = tmux.query(&query)?;
            match output.succeeded() {
                true => Ok(Self::first_line(&output)),
                false => Err(TmuxError::failed("show-options", &output).into()),
            }
        })
    }

    fn replaced_by(tmux: &TmuxCommand, keys: &[&HotKey]) -> Result<Self, MultiplexerError> {
        if keys.is_empty() {
            return Ok(Self::default());
        }
        let listings = Self::copy_table_listings(tmux)?;
        let slots = keys
            .iter()
            .flat_map(|key| {
                listings
                    .iter()
                    .map(move |(table, listing)| DisplacedBinding {
                        option: Self::option(table, key),
                        line: Self::listed_binding(listing, key),
                    })
            })
            .collect();
        Ok(Self { slots })
    }

    fn copy_table_listings(
        tmux: &TmuxCommand,
    ) -> Result<Vec<(&'static str, TmuxOutput)>, MultiplexerError> {
        TmuxServerConfig::COPY_TABLES
            .iter()
            .map(|table| {
                let query = TmuxSequence::new().command(["list-keys", "-T", table]);
                Ok((*table, tmux.query(&query)?))
            })
            .collect()
    }

    fn listed_binding(listing: &TmuxOutput, key: &HotKey) -> Option<String> {
        listing
            .stdout()
            .lines()
            .find(|line| Self::listed_key(line).and_then(HotKey::parse).as_ref() == Some(key))
            .map(str::to_string)
    }

    fn listed_key(line: &str) -> Option<&str> {
        let key = line
            .split_whitespace()
            .skip_while(|word| *word != Self::TABLE_FLAG)
            .nth(2)?;
        let quoted = key.strip_prefix('"').and_then(|key| key.strip_suffix('"'));
        Some(quoted.or_else(|| key.strip_prefix('\\')).unwrap_or(key))
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
        read: impl Fn(&str) -> Result<Option<String>, MultiplexerError>,
    ) -> Result<Self, MultiplexerError> {
        let mut slots = Vec::new();
        for key in keys {
            for table in TmuxServerConfig::COPY_TABLES {
                let option = Self::option(table, key);
                let line = read(&option)?;
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
    use crate::adapters::host::process_environment::ProcessEnvironment;
    use crate::adapters::tmux::server_files::ServerFiles;
    use crate::test_support::TmuxTestServer;

    #[test]
    fn a_listed_key_is_read_plain_quoted_or_escaped() {
        let lines = [
            "bind-key -T copy-mode M-x send-keys -X jump-to-mark",
            "bind-key -T copy-mode \"M-}\" send-keys -X next-paragraph",
            "bind-key -T copy-mode-vi \\; send-keys -X jump-again",
            "bind-key  -T copy-mode    Escape send-keys -X cancel",
            "bind-key -r -T prefix M-Up resize-pane -U 5",
        ];

        let keys = lines.map(DisplacedBindings::listed_key);

        assert_eq!(
            keys,
            [
                Some("M-x"),
                Some("M-}"),
                Some(";"),
                Some("Escape"),
                Some("M-Up")
            ]
        );
    }

    #[test]
    fn a_line_without_a_table_flag_lists_no_key() {
        assert_eq!(DisplacedBindings::listed_key("unknown key: C-q"), None);
    }

    #[test]
    fn a_switch_that_keeps_every_key_asks_tmux_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let files = ServerFiles::new(
            &directory.path().join("tmux.sock"),
            &directory.path().join("tmux.conf"),
        );
        let missing_program = directory.path().join("no-such-tmux");
        let tmux = TmuxCommand::new(
            missing_program.as_os_str(),
            &files,
            &ProcessEnvironment::default(),
            &TmuxTestServer::stand_in_finder(),
        );
        let bindings = KeyBindings::default();

        let switch = KeySwitch::read(&tmux, Some(&bindings), &bindings);

        assert!(switch.is_ok());
    }

    #[test]
    fn a_server_that_cannot_report_the_saved_bindings_fails_the_switch() {
        let server = TmuxTestServer::new();
        let tmux = server.command();
        let previous = KeyBindings::default().with_return_key(HotKey::parse("M-x").unwrap());

        let switch = KeySwitch::read(&tmux, Some(&previous), &KeyBindings::default());

        assert!(switch.is_err());
    }
}

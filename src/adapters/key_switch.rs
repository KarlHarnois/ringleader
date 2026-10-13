use super::tmux_command::TmuxCommand;
use super::tmux_error::TmuxError;
use super::tmux_output::TmuxOutput;
use super::tmux_sequence::TmuxSequence;
use super::tmux_server_config::TmuxServerConfig;
use crate::application::ports::MultiplexerError;
use crate::domain::ReturnKey;

pub struct KeySwitch {
    previous: ReturnKey,
    restored: DisplacedBindings,
    displaced: DisplacedBindings,
}

impl KeySwitch {
    pub fn read(
        tmux: &TmuxCommand,
        previous: &ReturnKey,
        next: &ReturnKey,
    ) -> Result<Self, MultiplexerError> {
        Ok(Self {
            previous: previous.clone(),
            restored: DisplacedBindings::saved_on(tmux)?,
            displaced: DisplacedBindings::replaced_by(tmux, next)?,
        })
    }

    pub fn script(&self) -> TmuxSequence {
        let previous = self.previous.to_string();
        TmuxServerConfig::return_tables()
            .filter(|table| !self.restored.holds(table))
            .fold(self.displaced.saving(), |sequence, table| {
                sequence.command(["unbind-key", "-q", "-T", table, &previous])
            })
    }

    pub fn restoring_lines(&self) -> String {
        self.restored.replay()
    }
}

struct DisplacedBindings {
    lines: Vec<(&'static str, String)>,
}

impl DisplacedBindings {
    const OPTION_PREFIX: &'static str = "@ringleader_displaced_";

    fn saved_on(tmux: &TmuxCommand) -> Result<Self, MultiplexerError> {
        Self::per_copy_table(|table| {
            let query = TmuxSequence::new().command(["show-options", "-gqv", &Self::option(table)]);
            let output = tmux.query(&query)?;
            match output.succeeded() {
                true => Ok(Self::first_line(&output)),
                false => Err(TmuxError::failed("show-options", &output).into()),
            }
        })
    }

    fn replaced_by(tmux: &TmuxCommand, key: &ReturnKey) -> Result<Self, MultiplexerError> {
        let key = key.to_string();
        Self::per_copy_table(|table| {
            let query = TmuxSequence::new().command(["list-keys", "-T", table, &key]);
            let output = tmux.query(&query)?;
            Ok(Self::first_line(&output).filter(|_| output.succeeded()))
        })
    }

    fn saving(&self) -> TmuxSequence {
        TmuxServerConfig::COPY_TABLES
            .iter()
            .fold(TmuxSequence::new(), |sequence, table| {
                let line = self.line(table).unwrap_or_default();
                sequence.command(["set-option", "-g", &Self::option(table), line])
            })
    }

    fn replay(&self) -> String {
        self.lines
            .iter()
            .map(|(_, line)| format!("{line}\n"))
            .collect()
    }

    fn holds(&self, table: &str) -> bool {
        self.line(table).is_some()
    }

    fn per_copy_table(
        read: impl Fn(&'static str) -> Result<Option<String>, MultiplexerError>,
    ) -> Result<Self, MultiplexerError> {
        let mut lines = Vec::new();
        for table in TmuxServerConfig::COPY_TABLES {
            if let Some(line) = read(table)? {
                lines.push((table, line));
            }
        }
        Ok(Self { lines })
    }

    fn first_line(output: &TmuxOutput) -> Option<String> {
        output
            .stdout()
            .lines()
            .next()
            .filter(|line| !line.is_empty())
            .map(str::to_string)
    }

    fn line(&self, table: &str) -> Option<&str> {
        self.lines
            .iter()
            .find(|(name, _)| *name == table)
            .map(|(_, line)| line.as_str())
    }

    fn option(table: &str) -> String {
        format!("{}{}", Self::OPTION_PREFIX, table.replace('-', "_"))
    }
}

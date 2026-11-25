use std::fmt::{self, Display, Formatter};
use std::io;
use std::path::Path;

use super::CliInput;
use super::prompt_text::PromptText;
use crate::application::ports::{PromptFileError, PromptFiles};

pub enum PromptArgument<'a> {
    Text(&'a str),
    File(&'a Path),
    StandardInput,
    Absent,
}

impl<'a> PromptArgument<'a> {
    const STANDARD_INPUT: &'static str = "standard input";
    const STANDARD_INPUT_FLAG: &'static str = "-";
    const PREFIX: &'static str = "--prompt-file";

    pub fn new(text: Option<&'a str>, file: Option<&'a str>) -> Self {
        match (text, file) {
            (Some(text), _) => Self::Text(text),
            (None, Some(Self::STANDARD_INPUT_FLAG)) => Self::StandardInput,
            (None, Some(path)) => Self::File(Path::new(path)),
            (None, None) => Self::Absent,
        }
    }

    pub fn read(
        &self,
        files: &dyn PromptFiles,
        input: &mut CliInput,
    ) -> Result<Option<PromptText>, PromptArgumentError> {
        match self {
            Self::Text(text) => Ok(Some(PromptText::new((*text).to_string()))),
            Self::File(path) => {
                Self::require_prompt(Self::read_file(files, path)?, &path.to_string_lossy())
                    .map(Some)
            }
            Self::StandardInput => {
                Self::require_prompt(Self::read_standard_input(input)?, Self::STANDARD_INPUT)
                    .map(Some)
            }
            Self::Absent => Ok(None),
        }
    }

    fn read_file(files: &dyn PromptFiles, path: &Path) -> Result<String, PromptArgumentError> {
        files
            .read_text(path)
            .map_err(|error| Self::file_error(path, error))
    }

    fn read_standard_input(input: &mut CliInput) -> Result<String, PromptArgumentError> {
        input.read_text().map_err(Self::input_error)
    }

    fn require_prompt(text: String, source: &str) -> Result<PromptText, PromptArgumentError> {
        PromptText::parse(text).ok_or_else(|| PromptArgumentError::NoPrompt {
            source: source.to_string(),
        })
    }

    fn input_error(error: io::Error) -> PromptArgumentError {
        let source = Self::STANDARD_INPUT.to_string();
        if error.kind() == io::ErrorKind::InvalidData {
            PromptArgumentError::NotText { source }
        } else {
            PromptArgumentError::Unreadable { source }
        }
    }

    fn file_error(path: &Path, error: PromptFileError) -> PromptArgumentError {
        let source = path.to_string_lossy().into_owned();
        match error {
            PromptFileError::Missing => PromptArgumentError::Missing { source },
            PromptFileError::Unreadable => PromptArgumentError::Unreadable { source },
            PromptFileError::NotText => PromptArgumentError::NotText { source },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptArgumentError {
    NoPrompt { source: String },
    Missing { source: String },
    Unreadable { source: String },
    NotText { source: String },
}

impl Display for PromptArgumentError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let prefix = PromptArgument::PREFIX;
        match self {
            Self::NoPrompt { source } => write!(formatter, "{prefix}: {source} holds no prompt"),
            Self::Missing { source } => write!(formatter, "{prefix}: no file at {source}"),
            Self::Unreadable { source } => write!(formatter, "{prefix}: cannot read {source}"),
            Self::NotText { source } => {
                write!(formatter, "{prefix}: {source} is not UTF-8 text")
            }
        }
    }
}

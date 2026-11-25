use std::fmt::{self, Display, Formatter};
use std::io;
use std::path::Path;

use super::CliInput;
use crate::application::ports::{PromptFileError, PromptFiles};

pub struct PromptArgument<'a> {
    text: Option<&'a str>,
    file: Option<&'a str>,
}

impl<'a> PromptArgument<'a> {
    const STANDARD_INPUT: &'static str = "standard input";
    const STANDARD_INPUT_FLAG: &'static str = "-";
    const PREFIX: &'static str = "--prompt-file";

    pub fn new(text: Option<&'a str>, file: Option<&'a str>) -> Self {
        Self { text, file }
    }

    pub fn resolve(
        &self,
        files: &dyn PromptFiles,
        input: &mut CliInput,
    ) -> Result<String, PromptArgumentError> {
        match (self.text, self.file) {
            (Some(text), _) => Ok(text.to_string()),
            (None, Some(Self::STANDARD_INPUT_FLAG)) => {
                let text = input.read_text().map_err(Self::input_error)?;
                Self::require_prompt(text, Self::STANDARD_INPUT)
            }
            (None, Some(path)) => {
                let text = files
                    .text(Path::new(path))
                    .map_err(|error| Self::file_error(path, error))?;
                Self::require_prompt(text, path)
            }
            (None, None) => Ok(String::new()),
        }
    }

    fn require_prompt(text: String, source: &str) -> Result<String, PromptArgumentError> {
        if text.trim().is_empty() {
            return Err(PromptArgumentError::NoPrompt {
                source: source.to_string(),
            });
        }
        Ok(Self::without_trailing_line_ends(&text).to_string())
    }

    fn without_trailing_line_ends(text: &str) -> &str {
        let mut end = text.len();
        while let Some(without_newline) = text[..end].strip_suffix('\n') {
            end = without_newline.len();
            if let Some(without_carriage_return) = text[..end].strip_suffix('\r') {
                end = without_carriage_return.len();
            }
        }
        &text[..end]
    }

    fn input_error(error: io::Error) -> PromptArgumentError {
        let source = Self::STANDARD_INPUT.to_string();
        if error.kind() == io::ErrorKind::InvalidData {
            PromptArgumentError::NotText { source }
        } else {
            PromptArgumentError::Unreadable { source }
        }
    }

    fn file_error(path: &str, error: PromptFileError) -> PromptArgumentError {
        let source = path.to_string();
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

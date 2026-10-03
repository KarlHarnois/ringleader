use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "ringleader",
    version,
    about = "A tmux session manager for Claude and opencode"
)]
pub struct Cli {
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    #[arg(long, global = true, value_name = "LEVEL")]
    log_level: Option<String>,
}

impl Cli {
    const INVALID_ARGUMENTS_EXIT: u8 = 1;

    pub fn parse_arguments<I, A>(arguments: I) -> Result<Self, ExitCode>
    where
        I: IntoIterator<Item = A>,
        A: Into<OsString> + Clone,
    {
        Self::try_parse_from(arguments).map_err(|error| {
            let _ = error.print();
            ExitCode::from(Self::exit_code_for(&error))
        })
    }

    pub fn config_path(&self) -> Option<&Path> {
        self.config.as_deref()
    }

    pub fn log_level(&self) -> Option<&str> {
        self.log_level.as_deref()
    }

    fn exit_code_for(error: &clap::Error) -> u8 {
        if error.use_stderr() {
            Self::INVALID_ARGUMENTS_EXIT
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ParseFixture;

    impl ParseFixture {
        fn exit_code(arguments: &[&str]) -> u8 {
            let error = Cli::try_parse_from(arguments).unwrap_err();
            Cli::exit_code_for(&error)
        }
    }

    #[test]
    fn an_unknown_flag_exits_one() {
        assert_eq!(
            ParseFixture::exit_code(&["ringleader", "--no-such-flag"]),
            1
        );
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(ParseFixture::exit_code(&["ringleader", "--help"]), 0);
    }

    #[test]
    fn the_config_option_is_accepted_without_a_subcommand() {
        let cli = Cli::try_parse_from(["ringleader", "--config", "/tmp/config.toml"]).unwrap();

        assert_eq!(cli.config_path(), Some(Path::new("/tmp/config.toml")));
    }
}

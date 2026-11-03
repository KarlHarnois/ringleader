mod complexity_counter;
mod crap_gate;
mod crap_limit;
mod crate_version;
mod function_collector;
mod gate_failure;
mod host_configuration;
mod instrumented_tests;
mod lcov_report;
mod line_coverage;
mod package_files;
mod release_version;
mod scored_function;
mod source_function;
mod source_tree;
mod toml_text;
mod version_bump;

use std::path::Path;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crap_gate::CrapGate;
use crate_version::CrateVersion;
use host_configuration::HostConfiguration;
use release_version::ReleaseVersion;
use version_bump::VersionBump;

#[derive(Parser)]
#[command(about = "Development tasks for this repository")]
struct Tasks {
    #[command(subcommand)]
    task: Task,
}

#[derive(Subcommand)]
enum Task {
    #[command(
        about = "Run every workspace test under coverage, then fail if a production function scores over the CRAP limit"
    )]
    Crap,
    #[command(
        about = "Print the tag that follows LATEST_TAG, bumped by the largest release label among LABELS"
    )]
    NextRelease {
        latest_tag: String,
        labels: Vec<String>,
    },
    #[command(about = "Write VERSION into the ringleader package of Cargo.toml and Cargo.lock")]
    SetVersion { version: String },
}

impl Task {
    fn run(self) -> ExitCode {
        let manifest_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_directory.parent().unwrap_or(manifest_directory);
        match self {
            Task::Crap => CrapGate::new(workspace_root, HostConfiguration::current()).run(),
            Task::NextRelease { latest_tag, labels } => {
                Self::print_next_release(&latest_tag, &labels)
            }
            Task::SetVersion { version } => Self::set_version(workspace_root, &version),
        }
    }

    fn print_next_release(latest_tag: &str, labels: &[String]) -> ExitCode {
        let bump = VersionBump::from_labels(labels.iter().map(String::as_str));
        match ReleaseVersion::parse_tag(latest_tag) {
            Ok(version) => {
                println!("{}", version.bumped(bump).tag());
                ExitCode::SUCCESS
            }
            Err(message) => Self::report(&message),
        }
    }

    fn set_version(workspace_root: &Path, version: &str) -> ExitCode {
        let written = ReleaseVersion::parse(version)
            .and_then(|version| CrateVersion::new(workspace_root).set(&version));
        match written {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => Self::report(&message),
        }
    }

    fn report(message: &str) -> ExitCode {
        eprintln!("{message}");
        ExitCode::FAILURE
    }
}

fn main() -> ExitCode {
    Tasks::parse().task.run()
}

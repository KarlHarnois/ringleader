mod complexity_counter;
mod crap_gate;
mod crap_limit;
mod function_collector;
mod gate_failure;
mod host_configuration;
mod instrumented_tests;
mod lcov_report;
mod line_coverage;
mod release_tag;
mod scored_function;
mod source_function;
mod source_tree;
mod version_bump;

use std::path::Path;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crap_gate::CrapGate;
use host_configuration::HostConfiguration;
use release_tag::ReleaseTag;
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
        }
    }

    fn print_next_release(latest_tag: &str, labels: &[String]) -> ExitCode {
        let bump = VersionBump::from_labels(labels.iter().map(String::as_str));
        match ReleaseTag::parse(latest_tag) {
            Ok(tag) => {
                println!("{}", tag.bumped(bump));
                ExitCode::SUCCESS
            }
            Err(message) => {
                eprintln!("{message}");
                ExitCode::FAILURE
            }
        }
    }
}

fn main() -> ExitCode {
    Tasks::parse().task.run()
}

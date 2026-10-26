mod complexity_counter;
mod crap_gate;
mod crap_limit;
mod function_collector;
mod gate_failure;
mod host_configuration;
mod instrumented_tests;
mod lcov_report;
mod line_coverage;
mod scored_function;
mod source_function;
mod source_tree;

use std::path::Path;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crap_gate::CrapGate;
use host_configuration::HostConfiguration;

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
}

impl Task {
    fn run(self) -> ExitCode {
        let manifest_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_directory.parent().unwrap_or(manifest_directory);
        match self {
            Task::Crap => CrapGate::new(workspace_root, HostConfiguration::current()).run(),
        }
    }
}

fn main() -> ExitCode {
    Tasks::parse().task.run()
}

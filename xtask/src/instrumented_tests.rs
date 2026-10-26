use std::fs;
use std::path::Path;
use std::process::{Command, ExitStatus};

use crate::gate_failure::GateFailure;
use crate::lcov_report::LcovReport;

pub struct InstrumentedTests<'a> {
    root: &'a Path,
}

impl<'a> InstrumentedTests<'a> {
    const REPORT: &'static str = "target/coverage/lcov.info";
    const UNMEASURED_PACKAGE: &'static str = "xtask";
    const CARGO: &'static str = "cargo";
    const FORGET_PREVIOUS_RUNS: [&'static str; 4] =
        ["llvm-cov", "clean", "--profraw-only", "--workspace"];
    const KEEP_BUILD_ARTIFACTS: &'static str = "--no-clean";

    pub fn new(root: &'a Path) -> Self {
        Self { root }
    }

    pub fn run(&self) -> Result<LcovReport, GateFailure> {
        let report = self.root.join(Self::REPORT);
        Self::create_parent(&report)?;
        self.cargo(GateFailure::coverage_unavailable, |command| {
            command.args(Self::FORGET_PREVIOUS_RUNS)
        })?;
        self.cargo(GateFailure::tests_failed, |command| {
            Self::measure(command, &report)
        })?;
        let text =
            fs::read_to_string(&report).map_err(|error| GateFailure::unreadable(&report, error))?;
        Ok(LcovReport::parse(&text, self.root))
    }

    fn create_parent(report: &Path) -> Result<(), GateFailure> {
        let Some(directory) = report.parent() else {
            return Ok(());
        };
        fs::create_dir_all(directory).map_err(|error| GateFailure::unwritable(directory, error))
    }

    fn measure<'command>(command: &'command mut Command, report: &Path) -> &'command mut Command {
        command
            .args(["llvm-cov", Self::KEEP_BUILD_ARTIFACTS, "--workspace"])
            .args(["--exclude-from-report", Self::UNMEASURED_PACKAGE])
            .args(["--lcov", "--output-path"])
            .arg(report)
    }

    fn cargo(
        &self,
        failure: fn(ExitStatus) -> GateFailure,
        arguments: impl FnOnce(&mut Command) -> &mut Command,
    ) -> Result<(), GateFailure> {
        let mut command = Command::new(Self::CARGO);
        command.current_dir(self.root);
        let status = arguments(&mut command)
            .status()
            .map_err(GateFailure::cargo_missing)?;
        if status.success() {
            Ok(())
        } else {
            Err(failure(status))
        }
    }
}

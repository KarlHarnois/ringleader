use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::crap_limit::CrapLimit;
use crate::gate_failure::GateFailure;
use crate::host_configuration::HostConfiguration;
use crate::instrumented_tests::InstrumentedTests;
use crate::lcov_report::LcovReport;
use crate::scored_function::ScoredFunction;
use crate::source_function::SourceFunction;
use crate::source_tree::SourceTree;

pub struct CrapGate {
    root: PathBuf,
    host: HostConfiguration,
}

impl CrapGate {
    const MAXIMUM_CRAP: f64 = 8.0;

    pub fn new(root: &Path, host: HostConfiguration) -> Self {
        Self {
            root: root.to_path_buf(),
            host,
        }
    }

    pub fn run(&self) -> ExitCode {
        match self.measure() {
            Ok(limit) => Self::judge(&limit),
            Err(failure) => failure.report(),
        }
    }

    fn measure(&self) -> Result<CrapLimit, GateFailure> {
        let report = InstrumentedTests::new(&self.root).run()?;
        let functions = SourceTree::new(&self.root, self.host).functions()?;
        Ok(CrapLimit::new(
            Self::MAXIMUM_CRAP,
            Self::score(functions, &report),
        ))
    }

    fn score(functions: Vec<SourceFunction>, report: &LcovReport) -> Vec<ScoredFunction> {
        functions
            .into_iter()
            .map(|function| {
                let coverage = report.within(function.file(), function.lines());
                ScoredFunction::new(function, coverage)
            })
            .collect()
    }

    fn judge(limit: &CrapLimit) -> ExitCode {
        if limit.passes() {
            println!("ok: {}", limit.summary());
            ExitCode::SUCCESS
        } else {
            println!("error: {}\n{}", limit.summary(), limit.details());
            ExitCode::FAILURE
        }
    }
}

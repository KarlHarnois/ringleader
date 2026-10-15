use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DoctorReport {
    lines: Vec<DoctorLine>,
}

impl DoctorReport {
    pub fn info(&mut self, message: impl Into<String>) {
        self.push(Severity::Info, message.into());
    }

    pub fn warning(&mut self, message: impl Into<String>) {
        self.push(Severity::Warning, message.into());
    }

    pub fn problem(&mut self, message: impl Into<String>) {
        self.push(Severity::Problem, message.into());
    }

    pub fn has_problems(&self) -> bool {
        self.lines
            .iter()
            .any(|line| line.severity == Severity::Problem)
    }

    pub fn lines(&self) -> Vec<String> {
        self.lines
            .iter()
            .map(|line| format!("{}: {}", line.severity.label(), line.message))
            .collect()
    }

    pub fn json(&self) -> String {
        serde_json::to_string_pretty(&self.lines).unwrap_or_default()
    }

    fn push(&mut self, severity: Severity, message: String) {
        self.lines.push(DoctorLine { severity, message });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct DoctorLine {
    severity: Severity,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Severity {
    Info,
    Warning,
    Problem,
}

impl Severity {
    fn label(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Problem => "problem",
        }
    }
}

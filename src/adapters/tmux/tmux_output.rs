#[derive(Debug, Clone)]
pub struct TmuxOutput {
    succeeded: bool,
    stdout: String,
    stderr: String,
}

impl TmuxOutput {
    const LABEL_PREFIX: &'static str = "ringleader-";
    const NO_SERVER: &'static str = "no server running on ";
    const NO_SOCKET: &'static str = "(No such file or directory)";
    const CONNECT_FAILURE: &'static str = "error connecting to ";
    const NO_SESSIONS: &'static str = "no current target";

    pub fn new(succeeded: bool, stdout: &str, stderr: &str) -> Self {
        Self {
            succeeded,
            stdout: stdout.to_string(),
            stderr: stderr.trim().to_string(),
        }
    }

    pub fn succeeded(&self) -> bool {
        self.succeeded
    }

    pub fn stdout(&self) -> &str {
        &self.stdout
    }

    pub fn stderr(&self) -> &str {
        &self.stderr
    }

    pub fn labeled(&self, label: &'static str) -> impl Iterator<Item = &str> {
        self.stdout
            .lines()
            .filter_map(move |line| Self::value_after(line, label))
    }

    pub fn first(&self, label: &'static str) -> Option<&str> {
        self.labeled(label).next()
    }

    pub fn has(&self, label: &'static str) -> bool {
        self.first(label).is_some()
    }

    pub fn unlabeled(&self) -> Vec<&str> {
        self.stdout
            .lines()
            .filter(|line| !line.starts_with(Self::LABEL_PREFIX))
            .collect()
    }

    pub fn reports_no_server(&self) -> bool {
        self.stderr.contains(Self::NO_SERVER)
            || (self.stderr.contains(Self::CONNECT_FAILURE)
                && self.stderr.contains(Self::NO_SOCKET))
    }

    pub fn reports_no_sessions(&self) -> bool {
        self.reports_no_server() || self.stderr.contains(Self::NO_SESSIONS)
    }

    fn value_after<'a>(line: &'a str, label: &str) -> Option<&'a str> {
        let rest = line.strip_prefix(label)?;
        if rest.is_empty() {
            return Some(rest);
        }
        rest.strip_prefix(' ').or_else(|| rest.strip_prefix('\t'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_read_by_label_and_hook_lines_are_ignored() {
        let output = TmuxOutput::new(false, "hook says hi\nringleader-done %3 0 /tmp/s\n", "");

        assert_eq!(output.first("ringleader-done"), Some("%3 0 /tmp/s"));
        assert_eq!(output.unlabeled(), ["hook says hi"]);
    }

    #[test]
    fn a_label_must_end_at_a_separator() {
        let output = TmuxOutput::new(true, "ringleader-done-later x\n", "");

        assert!(!output.has("ringleader-done"));
    }

    #[test]
    fn a_bare_label_counts() {
        let output = TmuxOutput::new(true, "ringleader-done\n", "");

        assert!(output.has("ringleader-done"));
    }

    #[test]
    fn an_absent_socket_reports_no_server() {
        let output = TmuxOutput::new(
            false,
            "",
            "error connecting to /tmp/rl/x.sock (No such file or directory)",
        );

        assert!(output.reports_no_server());
    }

    #[test]
    fn a_permission_error_is_not_a_missing_server() {
        let output = TmuxOutput::new(false, "", "error connecting to /tmp/x (Permission denied)");

        assert!(!output.reports_no_sessions());
    }
}

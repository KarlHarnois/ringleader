use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallKind {
    TmuxQuery,
    TmuxChange,
    GitDiff,
    GitRead,
    GitWrite,
    GhQuery,
}

#[derive(Debug, Clone, Copy)]
pub struct Deadlines {
    tmux_query: Duration,
    tmux_change: Duration,
    git_diff: Duration,
    git_read: Duration,
    git_write: Duration,
    gh_query: Duration,
}

impl Deadlines {
    pub fn standard() -> Self {
        Self {
            tmux_query: Duration::from_secs(2),
            tmux_change: Duration::from_secs(10),
            git_diff: Duration::from_secs(5),
            git_read: Duration::from_secs(60),
            git_write: Duration::from_secs(300),
            gh_query: Duration::from_secs(20),
        }
    }

    pub fn diff_poll() -> Self {
        let standard = Self::standard();
        Self {
            git_read: standard.git_diff,
            ..standard
        }
    }

    #[cfg(test)]
    pub fn uniform(deadline: Duration) -> Self {
        Self {
            tmux_query: deadline,
            tmux_change: deadline,
            git_diff: deadline,
            git_read: deadline,
            git_write: deadline,
            gh_query: deadline,
        }
    }

    pub fn for_kind(&self, kind: CallKind) -> Duration {
        match kind {
            CallKind::TmuxQuery => self.tmux_query,
            CallKind::TmuxChange => self.tmux_change,
            CallKind::GitDiff => self.git_diff,
            CallKind::GitRead => self.git_read,
            CallKind::GitWrite => self.git_write,
            CallKind::GhQuery => self.gh_query,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_diff_poll_reads_git_under_the_diff_deadline() {
        assert_eq!(
            Deadlines::diff_poll().for_kind(CallKind::GitRead),
            Deadlines::standard().for_kind(CallKind::GitDiff)
        );
    }
}

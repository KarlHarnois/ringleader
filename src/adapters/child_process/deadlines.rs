use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallKind {
    TmuxQuery,
    TmuxChange,
    GitDiff,
    GitRead,
    GitWrite,
    GitProbe,
    GhQuery,
    Direnv,
    TerminalQuery,
    UsageProbe,
}

impl CallKind {
    pub const ALL: [CallKind; 10] = [
        Self::TmuxQuery,
        Self::TmuxChange,
        Self::GitDiff,
        Self::GitRead,
        Self::GitWrite,
        Self::GitProbe,
        Self::GhQuery,
        Self::Direnv,
        Self::TerminalQuery,
        Self::UsageProbe,
    ];

    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Deadlines {
    by_kind: [Duration; CallKind::ALL.len()],
}

impl Deadlines {
    pub fn standard() -> Self {
        Self::uniform(Duration::ZERO)
            .with(CallKind::TmuxQuery, Duration::from_secs(2))
            .with(CallKind::TmuxChange, Duration::from_secs(10))
            .with(CallKind::GitDiff, Duration::from_secs(5))
            .with(CallKind::GitRead, Duration::from_secs(60))
            .with(CallKind::GitWrite, Duration::from_secs(300))
            .with(CallKind::GitProbe, Duration::from_secs(5))
            .with(CallKind::GhQuery, Duration::from_secs(20))
            .with(CallKind::Direnv, Duration::from_secs(10))
            .with(CallKind::TerminalQuery, Duration::from_secs(2))
            .with(CallKind::UsageProbe, Duration::from_secs(30))
    }

    pub fn diff_poll() -> Self {
        let standard = Self::standard();
        standard.with(CallKind::GitRead, standard.for_kind(CallKind::GitDiff))
    }

    pub fn uniform(deadline: Duration) -> Self {
        Self {
            by_kind: [deadline; CallKind::ALL.len()],
        }
    }

    pub fn for_kind(&self, kind: CallKind) -> Duration {
        self.by_kind[kind.index()]
    }

    fn with(self, kind: CallKind, deadline: Duration) -> Self {
        let mut by_kind = self.by_kind;
        by_kind[kind.index()] = deadline;
        Self { by_kind }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_probe_that_explains_a_failure_waits_less_than_a_read() {
        let standard = Deadlines::standard();

        assert!(standard.for_kind(CallKind::GitProbe) < standard.for_kind(CallKind::GitRead));
    }

    #[test]
    fn the_diff_poll_reads_git_under_the_diff_deadline() {
        assert_eq!(
            Deadlines::diff_poll().for_kind(CallKind::GitRead),
            Deadlines::standard().for_kind(CallKind::GitDiff)
        );
    }

    #[test]
    fn the_diff_poll_keeps_every_other_standard_deadline() {
        let other_kinds = CallKind::ALL
            .into_iter()
            .filter(|kind| *kind != CallKind::GitRead);

        for kind in other_kinds {
            assert_eq!(
                Deadlines::diff_poll().for_kind(kind),
                Deadlines::standard().for_kind(kind)
            );
        }
    }

    #[test]
    fn every_call_kind_has_a_standard_deadline() {
        for kind in CallKind::ALL {
            assert!(Deadlines::standard().for_kind(kind) > Duration::ZERO);
        }
    }

    #[test]
    fn every_call_kind_is_listed_in_its_place() {
        for (place, kind) in CallKind::ALL.into_iter().enumerate() {
            assert_eq!(kind.index(), place);
        }
    }

    #[test]
    fn a_uniform_deadline_holds_for_every_call_kind() {
        let deadline = Duration::from_secs(7);

        for kind in CallKind::ALL {
            assert_eq!(Deadlines::uniform(deadline).for_kind(kind), deadline);
        }
    }
}

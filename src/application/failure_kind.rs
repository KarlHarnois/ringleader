#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    Invalid,
    NotFound,
    Conflict,
    Unmeasurable,
    WouldLoseWork,
    TimedOut,
    LockHeld,
    Failed,
}

impl FailureKind {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid => "invalid",
            Self::NotFound => "not-found",
            Self::Conflict => "conflict",
            Self::Unmeasurable => "unmeasurable",
            Self::WouldLoseWork => "would-lose-work",
            Self::TimedOut => "timed-out",
            Self::LockHeld => "lock-held",
            Self::Failed => "failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_failure_kind_has_its_code() {
        let cases = [
            (FailureKind::Invalid, "invalid"),
            (FailureKind::NotFound, "not-found"),
            (FailureKind::Conflict, "conflict"),
            (FailureKind::Unmeasurable, "unmeasurable"),
            (FailureKind::WouldLoseWork, "would-lose-work"),
            (FailureKind::TimedOut, "timed-out"),
            (FailureKind::LockHeld, "lock-held"),
            (FailureKind::Failed, "failed"),
        ];

        for (kind, code) in cases {
            assert_eq!(kind.code(), code);
        }
    }
}

use crate::application::FailureKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliExit {
    Code(u8),
    Interrupted { signal: i32, code: u8 },
}

impl CliExit {
    pub const FAILURE: u8 = 1;
    pub const NOT_FOUND: u8 = 2;
    pub const CONFLICT: u8 = 3;

    pub fn exit_code(kind: FailureKind) -> u8 {
        match kind {
            FailureKind::NotFound => Self::NOT_FOUND,
            FailureKind::Conflict
            | FailureKind::Unmeasurable
            | FailureKind::WouldLoseWork
            | FailureKind::LockHeld => Self::CONFLICT,
            FailureKind::Invalid | FailureKind::TimedOut | FailureKind::Failed => Self::FAILURE,
        }
    }

    pub fn code(&self) -> u8 {
        match self {
            CliExit::Code(code) | CliExit::Interrupted { code, .. } => *code,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_failure_kind_has_its_exit_code() {
        let cases = [
            (FailureKind::Invalid, CliExit::FAILURE),
            (FailureKind::NotFound, CliExit::NOT_FOUND),
            (FailureKind::Conflict, CliExit::CONFLICT),
            (FailureKind::Unmeasurable, CliExit::CONFLICT),
            (FailureKind::WouldLoseWork, CliExit::CONFLICT),
            (FailureKind::TimedOut, CliExit::FAILURE),
            (FailureKind::LockHeld, CliExit::CONFLICT),
            (FailureKind::Failed, CliExit::FAILURE),
        ];

        for (kind, exit) in cases {
            assert_eq!(CliExit::exit_code(kind), exit);
        }
    }
}

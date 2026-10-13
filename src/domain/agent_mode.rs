#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentMode {
    #[default]
    Build,
    Plan,
}

impl AgentMode {
    pub fn toggled(self) -> Self {
        match self {
            Self::Build => Self::Plan,
            Self::Plan => Self::Build,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Plan => "plan",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_session_starts_in_build_mode() {
        assert_eq!(AgentMode::default(), AgentMode::Build);
    }

    #[test]
    fn toggling_build_gives_plan() {
        assert_eq!(AgentMode::Build.toggled(), AgentMode::Plan);
    }

    #[test]
    fn toggling_plan_gives_build() {
        assert_eq!(AgentMode::Plan.toggled(), AgentMode::Build);
    }
}

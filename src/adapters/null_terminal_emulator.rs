use std::path::Path;

use crate::application::ports::{TerminalEmulator, TerminalEmulatorError};

pub struct NullTerminalEmulator {
    missing: TerminalEmulatorError,
}

impl NullTerminalEmulator {
    pub fn new(missing: TerminalEmulatorError) -> Self {
        Self { missing }
    }
}

impl TerminalEmulator for NullTerminalEmulator {
    fn open_window(&self, _directory: &Path) -> Result<(), TerminalEmulatorError> {
        Err(self.missing.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_window_fails_with_what_the_host_lacks() {
        let missing = TerminalEmulatorError::NoDisplay {
            reason: "WAYLAND_DISPLAY and DISPLAY are unset or empty".to_string(),
        };
        let terminal = NullTerminalEmulator::new(missing.clone());

        assert_eq!(
            terminal.open_window(Path::new("/data/ringleader/worktrees/fix-auth")),
            Err(missing)
        );
    }
}

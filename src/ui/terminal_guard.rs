use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

pub trait ScreenMode: Send {
    fn enter(&mut self) -> io::Result<()>;
    fn leave(&mut self) -> io::Result<()>;
}

pub struct CrosstermScreenMode;

impl ScreenMode for CrosstermScreenMode {
    fn enter(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableBracketedPaste,
            EnableFocusChange
        )
    }

    fn leave(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();
        let _ = execute!(
            stdout,
            DisableFocusChange,
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
        let _ = stdout.flush();
        disable_raw_mode()
    }
}

pub struct TerminalGuard {
    state: Arc<TerminalState>,
}

impl TerminalGuard {
    pub fn enter(mode: Box<dyn ScreenMode>) -> io::Result<Self> {
        let state = Arc::new(TerminalState {
            in_tui_mode: AtomicBool::new(false),
            mode: Mutex::new(mode),
        });
        state.enter()?;
        Ok(Self { state })
    }

    pub fn state(&self) -> Arc<TerminalState> {
        Arc::clone(&self.state)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.state.restore();
    }
}

pub struct TerminalState {
    in_tui_mode: AtomicBool,
    mode: Mutex<Box<dyn ScreenMode>>,
}

impl TerminalState {
    pub fn install_panic_hook(self: &Arc<Self>) {
        let state = Arc::clone(self);
        let main_thread = std::thread::current().id();
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log::error!("panic: {info}");
            if std::thread::current().id() != main_thread {
                return;
            }
            state.restore();
            default_hook(info);
        }));
    }

    pub fn restore(&self) {
        if !self.in_tui_mode.swap(false, Ordering::SeqCst) {
            return;
        }
        if let Ok(mut mode) = self.mode.lock() {
            let _ = mode.leave();
        }
    }

    pub fn enter(&self) -> io::Result<()> {
        let mut mode = self
            .mode
            .lock()
            .map_err(|_| io::Error::other("the terminal state lock is poisoned"))?;
        mode.enter()?;
        self.in_tui_mode.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::RecordingScreenMode;

    #[test]
    fn entering_switches_to_tui_mode_once() {
        let mode = RecordingScreenMode::default();

        let _guard = TerminalGuard::enter(Box::new(mode.clone())).unwrap();

        assert_eq!(mode.switches(), ["enter"]);
    }

    #[test]
    fn a_restore_and_an_enter_leave_and_reenter_together() {
        let mode = RecordingScreenMode::default();
        let guard = TerminalGuard::enter(Box::new(mode.clone())).unwrap();

        guard.state().restore();
        guard.state().enter().unwrap();

        assert_eq!(mode.switches(), ["enter", "leave", "enter"]);
    }

    #[test]
    fn a_panic_restore_followed_by_the_drop_leaves_exactly_once() {
        let mode = RecordingScreenMode::default();
        let guard = TerminalGuard::enter(Box::new(mode.clone())).unwrap();
        let state = guard.state();

        state.restore();
        drop(guard);

        assert_eq!(mode.switches(), ["enter", "leave"]);
    }

    #[test]
    fn a_restore_outside_tui_mode_writes_nothing() {
        let mode = RecordingScreenMode::default();
        let guard = TerminalGuard::enter(Box::new(mode.clone())).unwrap();
        guard.state().restore();

        guard.state().restore();

        assert_eq!(mode.switches(), ["enter", "leave"]);
    }
}

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};

use crate::application::ports::Interrupts;

#[derive(Clone)]
pub struct SignalHookInterrupts {
    flags: Vec<(i32, Arc<AtomicBool>)>,
}

impl SignalHookInterrupts {
    const HANDLED: [i32; 4] = [SIGHUP, SIGINT, SIGTERM, SIGQUIT];

    pub fn register() -> io::Result<Self> {
        let flags = Self::unregistered();
        for (signal, flag) in &flags.flags {
            signal_hook::flag::register(*signal, Arc::clone(flag))?;
        }
        Ok(flags)
    }

    pub fn unregistered() -> Self {
        Self {
            flags: Self::HANDLED
                .iter()
                .map(|signal| (*signal, Arc::new(AtomicBool::new(false))))
                .collect(),
        }
    }

    #[cfg(test)]
    pub fn raise(&self, signal: i32) {
        if let Some((_, flag)) = self.flags.iter().find(|(handled, _)| *handled == signal) {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

impl Interrupts for SignalHookInterrupts {
    fn raised(&self) -> Option<i32> {
        self.flags
            .iter()
            .find(|(_, flag)| flag.load(Ordering::SeqCst))
            .map(|(signal, _)| *signal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_flag_is_raised_at_first() {
        assert_eq!(SignalHookInterrupts::unregistered().raised(), None);
    }

    #[test]
    fn a_raised_flag_names_its_signal() {
        let flags = SignalHookInterrupts::unregistered();

        flags.raise(SIGHUP);

        assert_eq!(flags.raised(), Some(SIGHUP));
    }
}

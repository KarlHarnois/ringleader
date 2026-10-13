use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::{Deadline, Gate};

pub struct HeldCalls {
    gate: Gate,
    arrivals: AtomicUsize,
    held: usize,
}

impl HeldCalls {
    const HOLD_LIMIT: Duration = Duration::from_secs(5);
    const ARRIVAL_LIMIT: Duration = Duration::from_secs(2);
    const QUIET: Duration = Duration::from_millis(200);

    pub fn first(held: usize) -> Arc<Self> {
        Arc::new(Self {
            gate: Gate::closed(),
            arrivals: AtomicUsize::new(0),
            held,
        })
    }

    pub fn all() -> Arc<Self> {
        Self::first(usize::MAX)
    }

    pub fn hook(self: &Arc<Self>) -> impl Fn() + Send + Sync + 'static {
        let calls = Arc::clone(self);
        move || calls.hold()
    }

    pub fn arrivals(&self) -> usize {
        self.arrivals.load(Ordering::SeqCst)
    }

    pub fn wait_for(&self, arrivals: usize) -> bool {
        Deadline::after(Self::ARRIVAL_LIMIT).wait_until(|| self.arrivals() >= arrivals)
    }

    pub fn stays_at(&self, arrivals: usize) -> bool {
        !Deadline::after(Self::QUIET).wait_until(|| self.arrivals() > arrivals)
    }

    pub fn release(&self) {
        self.gate.open_now();
    }

    fn hold(&self) {
        let arrival = self.arrivals.fetch_add(1, Ordering::SeqCst);
        if arrival < self.held {
            self.gate.wait_up_to(Self::HOLD_LIMIT);
        }
    }
}

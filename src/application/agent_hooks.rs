use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::pending::Pending;
use super::ports::{Multiplexer, MultiplexerError};
use crate::domain::{Agent, Session};

pub struct AgentHooks {
    multiplexer: Arc<dyn Multiplexer>,
    startup: Arc<Mutex<StartupJobs>>,
}

#[derive(Default)]
struct StartupJobs {
    in_flight: HashSet<String>,
    pending: Vec<Pending<()>>,
}

impl AgentHooks {
    pub fn new(multiplexer: Arc<dyn Multiplexer>) -> Self {
        Self {
            multiplexer,
            startup: Arc::new(Mutex::new(StartupJobs::default())),
        }
    }

    pub fn after_start(&self, session: &Session, agent: &Agent) {
        let lines = agent.startup_input(session.name());
        if lines.is_empty() {
            return;
        }
        self.queue_startup(session.tmux_name(), lines);
    }

    pub fn after_rename(&self, renamed: &Session, agent: &Agent) {
        let lines = agent.rename_input(renamed.name());
        if lines.is_empty() {
            return;
        }
        let typed = self.multiplexer.type_if_ready(renamed.tmux_name(), &lines);
        Self::log_skipped(renamed.tmux_name(), typed);
    }

    pub fn poll_startup_input(&self) {
        let mut jobs = self.jobs();
        jobs.pending.retain(|pending| pending.poll().is_none());
    }

    pub fn wait_startup_input(&self, stop: impl Fn() -> bool) {
        loop {
            let Some(pending) = self.jobs().pending.pop() else {
                return;
            };
            if pending.wait_unless(&stop).is_none() {
                return;
            }
        }
    }

    fn queue_startup(&self, tmux_name: &str, lines: Vec<String>) {
        let mut jobs = self.jobs();
        if !jobs.in_flight.insert(tmux_name.to_string()) {
            return;
        }
        let multiplexer = Arc::clone(&self.multiplexer);
        let startup = Arc::clone(&self.startup);
        let name = tmux_name.to_string();
        let pending = Pending::start(move || {
            let typed = multiplexer.type_when_ready(&name, &lines);
            Self::log_skipped(&name, typed);
            startup
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .in_flight
                .remove(&name);
            Ok(())
        });
        jobs.pending.push(pending);
    }

    fn jobs(&self) -> MutexGuard<'_, StartupJobs> {
        self.startup.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn log_skipped(tmux_name: &str, typed: Result<(), MultiplexerError>) {
        match typed {
            Ok(()) => {}
            Err(error @ MultiplexerError::AgentNotReady { .. }) => {
                log::info!("skipped typing into {tmux_name}: {error}");
            }
            Err(error) => log::warn!("could not type into {tmux_name}: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::test_support::{FakeMultiplexer, ServiceFixture};

    struct HooksFixture;

    impl HooksFixture {
        const TMUX_NAME: &'static str = "ringleader-fix-auth";
        const WAIT_LIMIT: Duration = Duration::from_secs(2);

        fn hooks(multiplexer: &Arc<FakeMultiplexer>) -> AgentHooks {
            let shared: Arc<dyn Multiplexer> = multiplexer.clone();
            AgentHooks::new(shared)
        }

        fn start_twice(hooks: &AgentHooks) {
            let session = ServiceFixture::session("fix-auth");
            let agent = ServiceFixture::agent_with_input();
            hooks.after_start(&session, &agent);
            hooks.after_start(&session, &agent);
        }

        fn settle(hooks: &AgentHooks) {
            let started = Instant::now();
            hooks.wait_startup_input(|| started.elapsed() > Self::WAIT_LIMIT);
        }
    }

    #[test]
    fn a_second_start_for_one_session_types_once() {
        let multiplexer = Arc::new(FakeMultiplexer::new().blocking_typing());
        let hooks = HooksFixture::hooks(&multiplexer);

        HooksFixture::start_twice(&hooks);
        multiplexer.release_typing();
        HooksFixture::settle(&hooks);

        assert_eq!(
            multiplexer.typed_when_ready(HooksFixture::TMUX_NAME),
            ["/color"]
        );
    }

    #[test]
    fn after_start_returns_while_the_typing_is_still_pending() {
        let multiplexer = Arc::new(FakeMultiplexer::new().blocking_typing());
        let hooks = HooksFixture::hooks(&multiplexer);
        let session = ServiceFixture::session("fix-auth");
        let started = Instant::now();

        hooks.after_start(&session, &ServiceFixture::agent_with_input());

        assert!(started.elapsed() < HooksFixture::WAIT_LIMIT);
        assert!(
            multiplexer
                .typed_when_ready(HooksFixture::TMUX_NAME)
                .is_empty()
        );
        multiplexer.release_typing();
        HooksFixture::settle(&hooks);
        assert_eq!(
            multiplexer.typed_when_ready(HooksFixture::TMUX_NAME),
            ["/color"]
        );
    }
}

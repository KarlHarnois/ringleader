use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event};
use ratatui::Terminal;
use ratatui::backend::Backend;

use super::app::{App, AppRequest};
use super::terminal_guard::TerminalState;
use crate::adapters::signal_flags::SignalFlags;

pub trait InputSource {
    fn next_event(&mut self, timeout: Duration) -> io::Result<Option<Event>>;
}

pub struct CrosstermInput;

impl InputSource for CrosstermInput {
    fn next_event(&mut self, timeout: Duration) -> io::Result<Option<Event>> {
        if event::poll(timeout)? {
            return event::read().map(Some);
        }
        Ok(None)
    }
}

pub struct EventLoop<B: Backend, I: InputSource> {
    terminal: Terminal<B>,
    input: I,
    signals: SignalFlags,
    screen: Arc<TerminalState>,
}

impl<B, I> EventLoop<B, I>
where
    B: Backend,
    B::Error: Send + Sync + 'static,
    I: InputSource,
{
    pub const TICK: Duration = Duration::from_millis(100);

    pub fn new(
        terminal: Terminal<B>,
        input: I,
        signals: SignalFlags,
        screen: Arc<TerminalState>,
    ) -> Self {
        Self {
            terminal,
            input,
            signals,
            screen,
        }
    }

    pub fn run(&mut self, app: &mut App) -> io::Result<()> {
        let mut next_tick = Instant::now();
        loop {
            if self.signals.raised().is_some() {
                return Ok(());
            }
            let now = Instant::now();
            if now >= next_tick {
                app.on_tick(now);
                next_tick = now + Self::TICK;
            }
            if let Err(error) = self.terminal.draw(|frame| app.render(frame)) {
                return self.unless_signaled(io::Error::other(error));
            }
            let timeout = next_tick.saturating_duration_since(Instant::now());
            let event = match self.input.next_event(timeout) {
                Ok(event) => event,
                Err(error) => return self.unless_signaled(error),
            };
            if let Some(event) = event
                && self.handle(app, &event)? == Flow::Stop
            {
                return Ok(());
            }
        }
    }

    #[cfg(test)]
    pub fn backend(&self) -> &B {
        self.terminal.backend()
    }

    fn handle(&mut self, app: &mut App, event: &Event) -> io::Result<Flow> {
        match app.handle_event(event) {
            AppRequest::Quit => return Ok(Flow::Stop),
            AppRequest::Redraw => self.clear()?,
            AppRequest::Attach(name) => self.attach(app, &name)?,
            AppRequest::Continue => {}
        }
        Ok(Flow::Continue)
    }

    fn attach(&mut self, app: &mut App, name: &str) -> io::Result<()> {
        if app.is_inside_tmux() {
            app.attach(name);
            return Ok(());
        }
        self.screen.restore();
        app.attach(name);
        self.screen.enter()?;
        self.clear()
    }

    fn clear(&mut self) -> io::Result<()> {
        self.terminal.clear().map_err(io::Error::other)
    }

    fn unless_signaled(&self, error: io::Error) -> io::Result<()> {
        match self.signals.raised() {
            Some(_) => Ok(()),
            None => Err(error),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Continue,
    Stop,
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;
    use signal_hook::consts::SIGHUP;

    use super::*;
    use crate::test_support::{
        FakeMultiplexer, FixtureBackend, ScriptedInput, ServiceFixture, UiHarness,
    };

    #[test]
    fn q_ends_the_loop_normally() {
        let harness = UiHarness::new();
        let mut app = UiHarness::app(&ServiceFixture::new());

        let result = harness
            .event_loop(ScriptedInput::events(vec![UiHarness::character('q')]))
            .run(&mut app);

        assert!(result.is_ok());
    }

    #[test]
    fn a_raised_signal_flag_ends_the_loop_normally() {
        let harness = UiHarness::new();
        let mut app = UiHarness::app(&ServiceFixture::new());
        harness.signals().raise(SIGHUP);

        let result = harness.event_loop(ScriptedInput::failing()).run(&mut app);

        assert!(result.is_ok());
    }

    #[test]
    fn an_input_error_without_a_signal_is_returned() {
        let harness = UiHarness::new();
        let mut app = UiHarness::app(&ServiceFixture::new());

        let result = harness.event_loop(ScriptedInput::failing()).run(&mut app);

        assert!(result.is_err());
    }

    #[test]
    fn an_input_error_after_a_raised_flag_ends_the_loop_normally() {
        let harness = UiHarness::new();
        let mut app = UiHarness::app(&ServiceFixture::new());
        let mut event_loop = harness.event_loop(ScriptedInput::failing());
        harness.signals().raise(SIGHUP);

        assert!(event_loop.run(&mut app).is_ok());
    }

    #[test]
    fn a_draw_error_after_a_raised_flag_ends_the_loop_normally() {
        let harness = UiHarness::new();
        let mut app = UiHarness::app(&ServiceFixture::new());
        let backend = FixtureBackend::new(UiHarness::WIDTH, UiHarness::HEIGHT).failing_draws();
        let mut event_loop = harness.event_loop_with(ScriptedInput::events(Vec::new()), backend);

        let unsignaled = event_loop.run(&mut app);
        harness.signals().raise(SIGHUP);
        let signaled = event_loop.run(&mut app);

        assert!(unsignaled.is_err());
        assert!(signaled.is_ok());
    }

    #[test]
    fn an_outside_attach_suspends_the_screen_and_redraws_fully_on_resume() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session)));
        let harness = UiHarness::new();
        let mut app = UiHarness::app(&fixture);
        let mut event_loop = harness.event_loop(ScriptedInput::events(vec![
            UiHarness::key(KeyCode::Enter),
            UiHarness::character('q'),
        ]));

        event_loop.run(&mut app).unwrap();

        assert_eq!(harness.screen_switches(), ["enter", "leave", "enter"]);
        assert_eq!(event_loop.backend().clears(), 1);
        assert_eq!(fixture.multiplexer().attached().len(), 1);
    }

    #[test]
    fn a_resize_forces_a_full_redraw() {
        let harness = UiHarness::new();
        let mut app = UiHarness::app(&ServiceFixture::new());
        let mut event_loop = harness.event_loop(ScriptedInput::events(vec![
            crossterm::event::Event::Resize(80, 24),
            UiHarness::character('q'),
        ]));

        event_loop.run(&mut app).unwrap();

        assert_eq!(event_loop.backend().clears(), 1);
    }

    #[test]
    fn the_session_table_lists_observed_sessions() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new().with_sessions(&[session]);
        let harness = UiHarness::new();
        let mut app = UiHarness::app(&fixture);
        let mut event_loop = harness.event_loop(ScriptedInput::events(vec![
            UiHarness::key(KeyCode::Down),
            UiHarness::key(KeyCode::Up),
            UiHarness::character('q'),
        ]));

        event_loop.run(&mut app).unwrap();

        assert!(event_loop.backend().text().contains("fix-auth"));
    }
}

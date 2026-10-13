use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event};
use ratatui::Terminal;
use ratatui::backend::Backend;

use super::app::{App, AppRequest};
use super::blocking_operation::BlockingOperation;
use super::shell_runner::{ChildShell, ShellRunner};
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
    shells: Box<dyn ShellRunner>,
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
            shells: Box::new(ChildShell),
        }
    }

    #[cfg(test)]
    pub fn with_shells(self, shells: Box<dyn ShellRunner>) -> Self {
        Self { shells, ..self }
    }

    pub fn run(&mut self, app: &mut App) -> io::Result<()> {
        let size = self.terminal.size().map_err(io::Error::other)?;
        app.resize(size.width, size.height);
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
            if let Err(error) = self.draw(app) {
                return self.unless_signaled(error);
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
            AppRequest::Shell(name) => self.open_shell(app, &name)?,
            AppRequest::Block(operation) => self.perform(app, operation)?,
            AppRequest::Continue => {}
        }
        Ok(Flow::Continue)
    }

    fn perform(&mut self, app: &mut App, operation: BlockingOperation) -> io::Result<()> {
        self.draw(app)?;
        app.perform(operation);
        self.drop_queued_input(app)
    }

    fn drop_queued_input(&mut self, app: &mut App) -> io::Result<()> {
        let mut last_resize = None;
        while let Some(event) = self.input.next_event(Duration::ZERO)? {
            if matches!(event, Event::Resize(_, _)) {
                last_resize = Some(event);
            }
        }
        if let Some(resize) = last_resize {
            self.handle(app, &resize)?;
        }
        Ok(())
    }

    fn draw(&mut self, app: &App) -> io::Result<()> {
        self.terminal
            .draw(|frame| app.render(frame))
            .map(drop)
            .map_err(io::Error::other)
    }

    fn attach(&mut self, app: &mut App, name: &str) -> io::Result<()> {
        self.screen.restore();
        app.attach(name);
        self.resume()
    }

    fn open_shell(&mut self, app: &mut App, name: &str) -> io::Result<()> {
        self.screen.restore();
        if let Some(shell) = app.open_shell(name) {
            app.shell_finished(self.shells.run(&shell));
        }
        self.resume()
    }

    fn resume(&mut self) -> io::Result<()> {
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

    use crate::application::EphemeralShell;
    use crate::application::ports::AttachTarget;
    use crate::test_support::{
        FakeMultiplexer, FixtureBackend, RecordingShells, ScriptedInput, ServiceFixture, UiHarness,
    };

    struct ShellLoopFixture;

    impl ShellLoopFixture {
        fn stopped() -> ServiceFixture {
            let session = ServiceFixture::session("fix-auth");
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_host(ServiceFixture::host_for(&[&session]))
        }

        fn live() -> ServiceFixture {
            let session = ServiceFixture::session("fix-auth");
            Self::stopped().with_multiplexer(
                FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session)),
            )
        }
    }

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
    fn an_attach_suspends_the_screen_and_redraws_fully_on_resume() {
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
    fn a_restart_draws_the_busy_message_before_it_runs() {
        let fixture = UiHarness::exited_session_fixture("fix-auth");
        let harness = UiHarness::new();
        let mut app = UiHarness::app(&fixture);
        let mut event_loop =
            harness.event_loop(ScriptedInput::events(vec![UiHarness::character('s')]));

        event_loop.run(&mut app).unwrap();

        let frames = event_loop.backend().frames();
        assert!(
            frames
                .iter()
                .any(|frame| frame.contains("Restarting fix-auth…"))
        );
        assert!(frames.last().unwrap().contains("Restarted fix-auth."));
    }

    #[test]
    fn keys_queued_during_a_restart_are_dropped() {
        let fixture = UiHarness::exited_session_fixture("fix-auth");
        let harness = UiHarness::new();
        let mut app = UiHarness::app(&fixture);
        let input =
            ScriptedInput::events(vec![UiHarness::character('s'), UiHarness::character('q')])
                .then_after_a_pause(vec![UiHarness::character('r')]);
        let mut event_loop = harness.event_loop(input);

        event_loop.run(&mut app).unwrap();

        let last_frame = event_loop.backend().frames().last().unwrap().clone();
        assert!(last_frame.contains("Reloaded the config and the registry."));
    }

    #[test]
    fn a_resize_queued_during_a_restart_is_kept() {
        let fixture = UiHarness::exited_session_fixture("fix-auth");
        let harness = UiHarness::new();
        let mut app = UiHarness::app(&fixture);
        let mut event_loop = harness.event_loop(ScriptedInput::events(vec![
            UiHarness::character('s'),
            crossterm::event::Event::Resize(UiHarness::WIDTH, UiHarness::HEIGHT),
        ]));

        event_loop.run(&mut app).unwrap();

        assert_eq!(event_loop.backend().clears(), 1);
    }

    #[test]
    fn an_enter_typed_while_delete_measures_cannot_confirm_the_dialog() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_workspace(ServiceFixture::workspace_for(&[&session]))
            .with_host(ServiceFixture::host_for(&[&session]))
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session)));
        let harness = UiHarness::new();
        let mut app = UiHarness::app(&fixture);
        let input = ScriptedInput::events(vec![
            UiHarness::character('d'),
            UiHarness::key(KeyCode::Enter),
        ])
        .then_after_a_pause(vec![UiHarness::key(KeyCode::Esc)]);
        let mut event_loop = harness.event_loop(input);

        event_loop.run(&mut app).unwrap();

        assert_eq!(fixture.registry().sessions().len(), 1);
    }

    #[test]
    fn t_attaches_to_the_shell_window_behind_a_suspended_screen() {
        let fixture = ShellLoopFixture::live();
        let harness = UiHarness::new();
        let mut app = UiHarness::app(&fixture);
        let mut event_loop =
            harness.event_loop(ScriptedInput::events(vec![UiHarness::character('t')]));

        event_loop.run(&mut app).unwrap();

        assert_eq!(harness.screen_switches(), ["enter", "leave", "enter"]);
        assert_eq!(
            fixture.multiplexer().attached(),
            [AttachTarget::Shell("@9".to_string())]
        );
    }

    #[test]
    fn t_on_a_stopped_session_runs_an_ephemeral_shell_in_its_worktree() {
        let fixture = ShellLoopFixture::stopped();
        let harness = UiHarness::new();
        let shells = RecordingShells::default();
        let mut app = UiHarness::app(&fixture);
        let mut event_loop = harness
            .event_loop(ScriptedInput::events(vec![UiHarness::character('t')]))
            .with_shells(Box::new(shells.clone()));

        event_loop.run(&mut app).unwrap();

        assert_eq!(
            shells.runs(),
            [EphemeralShell::new(
                ServiceFixture::SHELL,
                &ServiceFixture::worktree_path("fix-auth")
            )]
        );
        assert_eq!(harness.screen_switches(), ["enter", "leave", "enter"]);
        assert_eq!(event_loop.backend().clears(), 1);
    }

    #[test]
    fn a_shell_that_cannot_start_is_reported() {
        let fixture = ShellLoopFixture::stopped();
        let harness = UiHarness::new();
        let mut app = UiHarness::app(&fixture);
        let mut event_loop = harness
            .event_loop(ScriptedInput::events(vec![UiHarness::character('t')]))
            .with_shells(Box::new(RecordingShells::failing()));

        event_loop.run(&mut app).unwrap();

        let last_frame = event_loop.backend().frames().last().unwrap().clone();
        assert!(last_frame.contains("the shell failed:"));
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

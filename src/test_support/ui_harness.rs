use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::{Backend, ClearType, TestBackend, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};

use super::{
    Deadline, FakeInterrupts, FakeLock, FakeMultiplexer, FakeViewStore, RecordingScreenMode,
    ServiceFixture,
};
use crate::application::EphemeralShell;
use crate::domain::RepoGroup;
use crate::ui::App;
use crate::ui::event_loop::{EventLoop, InputSource};
use crate::ui::shell_runner::ShellRunner;
use crate::ui::terminal_guard::{TerminalGuard, TerminalState};

pub struct UiHarness {
    signals: FakeInterrupts,
    mode: RecordingScreenMode,
    guard: TerminalGuard,
}

impl UiHarness {
    pub const WIDTH: u16 = 100;
    pub const HEIGHT: u16 = 28;
    const DOCS_REPO: &'static str = "/repos/alpha";

    pub fn new() -> Self {
        let mode = RecordingScreenMode::default();
        let guard = TerminalGuard::enter(Box::new(mode.clone())).expect("a recording screen mode");
        Self {
            signals: FakeInterrupts::default(),
            mode,
            guard,
        }
    }

    pub fn exited_session_fixture(name: &str) -> ServiceFixture {
        let session = ServiceFixture::session(name);
        ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_workspace(ServiceFixture::workspace_for(&[&session]))
            .with_host(ServiceFixture::host_for(&[&session]))
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::exited(&session)))
    }

    pub fn two_repos() -> ServiceFixture {
        let docs = ServiceFixture::session_at("docs", Self::DOCS_REPO, ServiceFixture::CREATED_AT);
        let fix_auth = ServiceFixture::session("fix-auth");
        ServiceFixture::new()
            .with_sessions(&[docs.clone(), fix_auth.clone()])
            .with_multiplexer(
                FakeMultiplexer::new()
                    .with_live(ServiceFixture::waiting(&docs))
                    .with_live(ServiceFixture::waiting(&fix_auth)),
            )
    }

    pub fn docs_repo_group() -> RepoGroup {
        ServiceFixture::repo_group(Self::DOCS_REPO)
    }

    pub fn app(fixture: &ServiceFixture) -> App {
        Self::app_with_view(fixture, &FakeViewStore::default())
    }

    pub fn app_with_view(fixture: &ServiceFixture, view: &FakeViewStore) -> App {
        Self::assembled(fixture, view, &FakeLock::free())
    }

    pub fn app_with_lock(fixture: &ServiceFixture, lock: &FakeLock) -> App {
        Self::assembled(fixture, &FakeViewStore::default(), lock)
    }

    fn assembled(fixture: &ServiceFixture, view: &FakeViewStore, lock: &FakeLock) -> App {
        App::new(
            fixture.service(),
            Box::new(lock.clone()),
            Box::new(io::sink()),
            Box::new(view.clone()),
            Instant::now(),
        )
    }

    pub fn settle(app: &mut App) {
        Deadline::for_background_work().wait_until(|| {
            app.on_tick(Instant::now());
            !app.awaits_results()
        });
    }

    pub fn signals(&self) -> &FakeInterrupts {
        &self.signals
    }

    pub fn screen_switches(&self) -> Vec<&'static str> {
        self.mode.switches()
    }

    pub fn screen(&self) -> Arc<TerminalState> {
        self.guard.state()
    }

    pub fn event_loop(&self, input: ScriptedInput) -> EventLoop<FixtureBackend, ScriptedInput> {
        self.event_loop_with(input, FixtureBackend::new(Self::WIDTH, Self::HEIGHT))
    }

    pub fn event_loop_with(
        &self,
        input: ScriptedInput,
        backend: FixtureBackend,
    ) -> EventLoop<FixtureBackend, ScriptedInput> {
        let terminal = Terminal::new(backend).expect("a fixture terminal");
        EventLoop::new(
            terminal,
            input,
            Box::new(self.signals.clone()),
            self.screen(),
        )
    }

    pub fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    pub fn character(character: char) -> Event {
        Self::key(KeyCode::Char(character))
    }

    pub fn control(character: char) -> Event {
        Event::Key(KeyEvent::new(
            KeyCode::Char(character),
            KeyModifiers::CONTROL,
        ))
    }
}

#[derive(Clone, Default)]
pub struct RecordingShells {
    runs: Arc<Mutex<Vec<EphemeralShell>>>,
    failing: bool,
}

impl RecordingShells {
    pub fn failing() -> Self {
        Self {
            failing: true,
            ..Self::default()
        }
    }

    pub fn runs(&self) -> Vec<EphemeralShell> {
        self.runs.lock().unwrap().clone()
    }
}

impl ShellRunner for RecordingShells {
    fn run(&mut self, shell: &EphemeralShell) -> io::Result<()> {
        self.runs.lock().unwrap().push(shell.clone());
        if self.failing {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        Ok(())
    }
}

pub struct ScriptedInput {
    events: VecDeque<io::Result<Option<Event>>>,
    timeouts: Vec<Duration>,
}

impl ScriptedInput {
    pub fn events(events: Vec<Event>) -> Self {
        Self {
            events: events.into_iter().map(|event| Ok(Some(event))).collect(),
            timeouts: Vec::new(),
        }
    }

    pub fn timeouts(&self) -> &[Duration] {
        &self.timeouts
    }

    pub fn then_after_a_pause(mut self, events: Vec<Event>) -> Self {
        self.events.push_back(Ok(None));
        self.events
            .extend(events.into_iter().map(|event| Ok(Some(event))));
        self
    }

    pub fn failing() -> Self {
        Self {
            events: VecDeque::from([Err(io::Error::other("the terminal hung up"))]),
            timeouts: Vec::new(),
        }
    }
}

impl InputSource for ScriptedInput {
    fn next_event(&mut self, timeout: Duration) -> io::Result<Option<Event>> {
        self.timeouts.push(timeout);
        match self.events.pop_front() {
            Some(event) => event,
            None if timeout.is_zero() => Ok(None),
            None => Ok(Some(UiHarness::character('q'))),
        }
    }
}

pub struct FixtureBackend {
    inner: TestBackend,
    failing_draws: bool,
    clears: usize,
    frames: Vec<String>,
}

impl FixtureBackend {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            inner: TestBackend::new(width, height),
            failing_draws: false,
            clears: 0,
            frames: Vec::new(),
        }
    }

    pub fn failing_draws(self) -> Self {
        Self {
            failing_draws: true,
            ..self
        }
    }

    pub fn clears(&self) -> usize {
        self.clears
    }

    pub fn frames(&self) -> &[String] {
        &self.frames
    }

    pub fn text(&self) -> String {
        let buffer = self.inner.buffer();
        let width = usize::from(buffer.area.width);
        buffer
            .content()
            .chunks(width)
            .map(|row| row.iter().map(Cell::symbol).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Backend for FixtureBackend {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        if self.failing_draws {
            return Err(io::Error::other("the terminal is gone"));
        }
        self.inner.draw(content).map_err(io::Error::other)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.inner.hide_cursor().map_err(io::Error::other)
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.inner.show_cursor().map_err(io::Error::other)
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        self.inner.get_cursor_position().map_err(io::Error::other)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.inner
            .set_cursor_position(position)
            .map_err(io::Error::other)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.clears += 1;
        self.inner.clear().map_err(io::Error::other)
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        if clear_type == ClearType::All {
            self.clears += 1;
        }
        self.inner
            .clear_region(clear_type)
            .map_err(io::Error::other)
    }

    fn size(&self) -> io::Result<Size> {
        self.inner.size().map_err(io::Error::other)
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        self.inner.window_size().map_err(io::Error::other)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.frames.push(self.text());
        self.inner.flush().map_err(io::Error::other)
    }
}

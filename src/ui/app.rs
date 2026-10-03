use std::time::Instant;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};

use super::home_view::HomeView;
use super::status_bar::StatusBar;
use super::theme::Theme;
use crate::application::{ReturnMode, SessionService};
use crate::domain::SessionSummary;

pub struct App {
    service: SessionService,
    rows: Vec<SessionSummary>,
    selection: Option<String>,
    message: Option<String>,
    tick: u64,
    next_observe: Instant,
    theme: Theme,
}

impl App {
    pub fn new(service: SessionService, now: Instant) -> Self {
        Self {
            service,
            rows: Vec::new(),
            selection: None,
            message: None,
            tick: 0,
            next_observe: now,
            theme: Theme,
        }
    }

    pub fn with_message(self, message: Option<String>) -> Self {
        Self { message, ..self }
    }

    pub fn is_inside_tmux(&self) -> bool {
        self.service.is_inside_tmux()
    }

    pub fn handle_event(&mut self, event: &Event) -> AppRequest {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.handle_key(key),
            Event::Resize(_, _) => AppRequest::Redraw,
            _ => AppRequest::Continue,
        }
    }

    pub fn on_tick(&mut self, now: Instant) {
        self.tick = self.tick.wrapping_add(1);
        if now < self.next_observe {
            return;
        }
        self.next_observe = now + self.service.config().intervals().state_poll();
        self.observe();
    }

    pub fn attach(&mut self, name: &str) {
        match self.service.attach(name, ReturnMode::None) {
            Ok(outcome) => self.message = outcome.binding_warning().map(str::to_string),
            Err(error) => self.message = Some(error.to_string()),
        }
        self.observe();
    }

    pub fn render(&self, frame: &mut Frame) {
        let [table, status] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
        HomeView::new(&self.rows, self.selection.as_deref(), self.tick, self.theme)
            .render(frame, table);
        StatusBar::new(self.message.as_deref(), self.theme).render(frame, status);
    }

    fn handle_key(&mut self, key: &KeyEvent) -> AppRequest {
        match key.code {
            KeyCode::Char('q') => AppRequest::Quit,
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Enter => match &self.selection {
                Some(name) => AppRequest::Attach(name.clone()),
                None => AppRequest::Continue,
            },
            _ => AppRequest::Continue,
        }
    }

    fn move_selection(&mut self, offset: isize) -> AppRequest {
        let current = self.selected_index().unwrap_or(0);
        let last = self.rows.len().saturating_sub(1);
        let target = current.saturating_add_signed(offset).min(last);
        self.selection = self.rows.get(target).map(|row| row.name().to_string());
        AppRequest::Continue
    }

    fn selected_index(&self) -> Option<usize> {
        let selected = self.selection.as_deref()?;
        self.rows.iter().position(|row| row.name() == selected)
    }

    fn observe(&mut self) {
        let persist = self.service.read_only_reason().is_none();
        match self.service.observe(persist) {
            Ok(rows) => self.show(rows),
            Err(error) => self.message = Some(error.to_string()),
        }
    }

    fn show(&mut self, mut rows: Vec<SessionSummary>) {
        rows.sort_by(|first, second| {
            (first.state(), first.name()).cmp(&(second.state(), second.name()))
        });
        self.rows = rows;
        if self.selected_index().is_none() {
            self.selection = self.rows.first().map(|row| row.name().to_string());
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppRequest {
    Continue,
    Redraw,
    Quit,
    Attach(String),
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Instant;

    use super::*;
    use crate::application::ports::ConfigError;
    use crate::test_support::{FakeMultiplexer, ServiceFixture, UiHarness};

    #[test]
    fn a_tui_on_a_broken_config_observes_without_persisting() {
        let adoptable = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_broken_config(ConfigError::Parse {
                path: PathBuf::from("/config/ringleader/config.toml"),
                detail: "expected `=`".to_string(),
            })
            .with_multiplexer(
                FakeMultiplexer::new().with_live(ServiceFixture::waiting(&adoptable)),
            );
        let mut app = UiHarness::app(&fixture);

        app.on_tick(Instant::now());

        assert_eq!(fixture.registry().save_count(), 0);
    }

    #[test]
    fn a_tui_on_a_valid_config_persists_an_adoption() {
        let adoptable = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new().with_live(ServiceFixture::waiting(&adoptable)),
        );
        let mut app = UiHarness::app(&fixture);

        app.on_tick(Instant::now());

        assert_eq!(fixture.registry().save_count(), 1);
    }
}

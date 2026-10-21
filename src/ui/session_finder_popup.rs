use std::io;
use std::time::Duration;

use crossterm::event::{Event, KeyEvent};
use ratatui::Terminal;
use ratatui::backend::Backend;
use thiserror::Error;

use super::app::AppRequest;
use super::dialogs::{Dialog, DialogStep};
use super::event_loop::InputSource;
use super::keys::back_key::BackKey;
use super::keys::control_key::ControlKey;
use super::table::session_list::SessionList;
use super::theme::Theme;
use crate::application::{CorruptRegistry, ServiceError, SessionService};

pub struct SessionFinderPopup {
    service: SessionService,
    client: String,
}

impl SessionFinderPopup {
    const IDLE_WAIT: Duration = Duration::from_secs(60);
    const INTERRUPT: ControlKey = ControlKey::new('c');

    pub fn new(service: SessionService, client: &str) -> Self {
        Self {
            service,
            client: client.to_string(),
        }
    }

    pub fn run<B, I>(mut self, backend: B, mut input: I) -> Result<(), FinderPopupError>
    where
        B: Backend,
        B::Error: Send + Sync + 'static,
        I: InputSource,
    {
        let mut terminal = Terminal::new(backend).map_err(io::Error::other)?;
        let mut finder = Dialog::find_session(self.load_sessions()?.rows());
        match self.choose(&mut terminal, &mut input, &mut finder)? {
            Choice::Session(name) => Ok(self.service.switch_client(&name, &self.client)?),
            Choice::Dismissed => Ok(()),
        }
    }

    fn load_sessions(&mut self) -> Result<SessionList, ServiceError> {
        self.service.load_registry(CorruptRegistry::Report)?;
        let mut sessions = SessionList::default();
        sessions.show(self.service.observe(false)?);
        Ok(sessions)
    }

    fn choose<B, I>(
        &self,
        terminal: &mut Terminal<B>,
        input: &mut I,
        finder: &mut Dialog,
    ) -> Result<Choice, FinderPopupError>
    where
        B: Backend,
        B::Error: Send + Sync + 'static,
        I: InputSource,
    {
        loop {
            terminal
                .draw(|frame| finder.fill(frame, &Theme))
                .map_err(io::Error::other)?;
            let event = input.next_event(Self::IDLE_WAIT)?;
            if let Some(choice) = event.and_then(|event| self.answer(finder, &event)) {
                return Ok(choice);
            }
        }
    }

    fn answer(&self, finder: &mut Dialog, event: &Event) -> Option<Choice> {
        if let Event::Key(key) = event
            && Self::dismisses(key)
        {
            return Some(Choice::Dismissed);
        }
        match finder.handle_event(event, &self.service) {
            DialogStep::CloseWith(AppRequest::Attach(name)) => Some(Choice::Session(name)),
            DialogStep::Close => Some(Choice::Dismissed),
            DialogStep::Stay | DialogStep::Request(_) | DialogStep::CloseWith(_) => None,
        }
    }

    fn dismisses(key: &KeyEvent) -> bool {
        BackKey::matches(key) || Self::INTERRUPT.matches(key)
    }
}

enum Choice {
    Session(String),
    Dismissed,
}

#[derive(Debug, Error)]
pub enum FinderPopupError {
    #[error("the session finder lost its terminal: {0}")]
    Terminal(#[from] io::Error),
    #[error(transparent)]
    Service(#[from] ServiceError),
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::application::ports::AttachTarget;
    use crate::test_support::{
        FakeMultiplexer, FixtureBackend, ScriptedInput, ServiceFixture, UiHarness,
    };

    struct PopupFixture;

    impl PopupFixture {
        const CLIENT: &'static str = "client-1234";

        fn run(fixture: &ServiceFixture, events: Vec<Event>) -> Result<(), FinderPopupError> {
            let backend = FixtureBackend::new(UiHarness::WIDTH / 2, UiHarness::HEIGHT / 2);
            let script = ScriptedInput::events(Self::ending_dismissed(events));
            SessionFinderPopup::new(fixture.service(), Self::CLIENT).run(backend, script)
        }

        fn ending_dismissed(mut events: Vec<Event>) -> Vec<Event> {
            events.push(Self::control('c'));
            events
        }

        fn typed(text: &str) -> Vec<Event> {
            text.chars().map(UiHarness::character).collect()
        }

        fn control(character: char) -> Event {
            Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::CONTROL,
            ))
        }

        fn enter() -> Event {
            UiHarness::key(KeyCode::Enter)
        }

        fn switched_to(name: &str) -> Vec<(String, AttachTarget)> {
            vec![(
                Self::CLIENT.to_string(),
                AttachTarget::Agent(format!("ringleader-{name}")),
            )]
        }

        fn stopped(name: &str) -> ServiceFixture {
            ServiceFixture::new().with_sessions(&[ServiceFixture::session(name)])
        }

        fn live(name: &str) -> ServiceFixture {
            let session = ServiceFixture::session(name);
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_multiplexer(
                    FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session)),
                )
        }
    }

    #[test]
    fn enter_switches_the_client_to_the_first_session_in_the_tuis_order() {
        let fixture = UiHarness::two_repos();

        PopupFixture::run(&fixture, vec![PopupFixture::enter()]).unwrap();

        assert_eq!(
            fixture.multiplexer().switched(),
            PopupFixture::switched_to("docs")
        );
    }

    #[test]
    fn a_typed_query_picks_the_session_it_matches() {
        let fixture = UiHarness::two_repos();
        let mut events = PopupFixture::typed("fix");
        events.push(PopupFixture::enter());

        PopupFixture::run(&fixture, events).unwrap();

        assert_eq!(
            fixture.multiplexer().switched(),
            PopupFixture::switched_to("fix-auth")
        );
    }

    #[test]
    fn a_typed_repo_name_picks_a_session_in_that_repo() {
        let fixture = UiHarness::two_repos();
        let mut events = PopupFixture::typed("example");
        events.push(PopupFixture::enter());

        PopupFixture::run(&fixture, events).unwrap();

        assert_eq!(
            fixture.multiplexer().switched(),
            PopupFixture::switched_to("fix-auth")
        );
    }

    #[test]
    fn escape_ctrl_c_and_ctrl_q_close_the_finder_without_a_switch() {
        for key in [
            UiHarness::key(KeyCode::Esc),
            PopupFixture::control('c'),
            PopupFixture::control('q'),
        ] {
            let fixture = UiHarness::two_repos();

            PopupFixture::run(&fixture, vec![key.clone()]).unwrap();

            assert!(fixture.multiplexer().switched().is_empty(), "{key:?}");
        }
    }

    #[test]
    fn a_pasted_name_picks_the_session_it_matches() {
        let fixture = UiHarness::two_repos();
        let events = vec![Event::Paste("fix-auth".to_string()), PopupFixture::enter()];

        PopupFixture::run(&fixture, events).unwrap();

        assert_eq!(
            fixture.multiplexer().switched(),
            PopupFixture::switched_to("fix-auth")
        );
    }

    #[test]
    fn the_popup_takes_no_lock_so_it_never_writes_the_registry() {
        let unregistered = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new().with_live(ServiceFixture::waiting(&unregistered)),
        );

        PopupFixture::run(&fixture, vec![UiHarness::key(KeyCode::Esc)]).unwrap();

        assert_eq!(fixture.registry().save_count(), 0);
    }

    #[test]
    fn picking_a_stopped_session_returns_the_refusal() {
        let fixture = PopupFixture::stopped("fix-auth");

        let error = PopupFixture::run(&fixture, vec![PopupFixture::enter()]).unwrap_err();

        assert!(error.to_string().contains("stopped"), "{error}");
    }

    #[test]
    fn a_lost_terminal_is_returned() {
        let fixture = PopupFixture::live("fix-auth");
        let backend = FixtureBackend::new(UiHarness::WIDTH, UiHarness::HEIGHT);

        let outcome = SessionFinderPopup::new(fixture.service(), PopupFixture::CLIENT)
            .run(backend, ScriptedInput::failing());

        assert!(matches!(outcome, Err(FinderPopupError::Terminal(_))));
    }
}

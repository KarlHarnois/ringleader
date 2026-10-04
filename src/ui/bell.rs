use std::collections::HashMap;
use std::io::Write;

use crate::application::{ServiceError, SessionService};
use crate::domain::{SessionState, SessionSummary};

pub struct Bell {
    previous: HashMap<String, SessionState>,
    sink: Box<dyn Write>,
}

impl Bell {
    const BEL: &'static [u8] = b"\x07";

    pub fn new(sink: Box<dyn Write>) -> Self {
        Self {
            previous: HashMap::new(),
            sink,
        }
    }

    pub fn watch(
        &mut self,
        rows: &[SessionSummary],
        service: &SessionService,
    ) -> Result<(), ServiceError> {
        let finished_working = rows.iter().any(|row| self.finished_working(row));
        self.previous = rows
            .iter()
            .map(|row| (row.name().to_string(), row.state()))
            .collect();
        if !finished_working || !service.config().bell_on_waiting() {
            return Ok(());
        }
        if service.is_inside_tmux() {
            return service.ring_bell();
        }
        let _ = self.sink.write_all(Self::BEL);
        let _ = self.sink.flush();
        Ok(())
    }

    fn finished_working(&self, row: &SessionSummary) -> bool {
        let previous = self.previous.get(row.name()).copied();
        let edge =
            previous.is_some_and(|previous| SessionState::is_bell_edge(previous, row.state()));
        edge && !row.attached()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Config, ConfigDraft};
    use crate::test_support::{CapturedWriter, ServiceFixture};

    struct BellFixture;

    impl BellFixture {
        fn row(state: SessionState, attached: bool) -> SessionSummary {
            SessionSummary::managed(ServiceFixture::session("fix-auth"), state, attached)
        }

        fn ringing(fixture: ServiceFixture) -> ServiceFixture {
            let draft = ConfigDraft::new(std::path::Path::new(ServiceFixture::WORKTREE_ROOT))
                .bell_on_waiting(true)
                .agent(ServiceFixture::agent());
            fixture.with_config(Config::new(&draft).unwrap())
        }

        fn rings(
            fixture: &ServiceFixture,
            previous: SessionSummary,
            current: SessionSummary,
        ) -> String {
            let writer = CapturedWriter::default();
            let mut bell = Bell::new(Box::new(writer.clone()));
            let service = fixture.service();
            bell.watch(&[previous], &service).unwrap();
            bell.watch(&[current], &service).unwrap();
            writer.text()
        }
    }

    #[test]
    fn working_to_waiting_rings_the_terminal_outside_tmux() {
        let fixture = BellFixture::ringing(ServiceFixture::new());

        let rung = BellFixture::rings(
            &fixture,
            BellFixture::row(SessionState::Working, false),
            BellFixture::row(SessionState::Waiting, false),
        );

        assert_eq!(rung, "\u{7}");
    }

    #[test]
    fn an_attached_session_does_not_ring() {
        let fixture = BellFixture::ringing(ServiceFixture::new());

        let rung = BellFixture::rings(
            &fixture,
            BellFixture::row(SessionState::Working, true),
            BellFixture::row(SessionState::Waiting, true),
        );

        assert_eq!(rung, "");
    }

    #[test]
    fn an_exit_does_not_ring() {
        let fixture = BellFixture::ringing(ServiceFixture::new());

        let rung = BellFixture::rings(
            &fixture,
            BellFixture::row(SessionState::Working, false),
            BellFixture::row(SessionState::Exited, false),
        );

        assert_eq!(rung, "");
    }

    #[test]
    fn the_bell_is_off_by_default() {
        let rung = BellFixture::rings(
            &ServiceFixture::new(),
            BellFixture::row(SessionState::Working, false),
            BellFixture::row(SessionState::Waiting, false),
        );

        assert_eq!(rung, "");
    }

    #[test]
    fn the_first_observation_never_rings() {
        let fixture = BellFixture::ringing(ServiceFixture::new());
        let writer = CapturedWriter::default();
        let mut bell = Bell::new(Box::new(writer.clone()));

        bell.watch(
            &[BellFixture::row(SessionState::Waiting, false)],
            &fixture.service(),
        )
        .unwrap();

        assert_eq!(writer.text(), "");
    }

    #[test]
    fn inside_tmux_the_bell_rings_every_client_through_tmux() {
        let fixture = BellFixture::ringing(ServiceFixture::new().inside_tmux());

        let rung = BellFixture::rings(
            &fixture,
            BellFixture::row(SessionState::Working, false),
            BellFixture::row(SessionState::Waiting, false),
        );

        assert_eq!(rung, "");
        assert_eq!(fixture.multiplexer().bells(), 1);
    }

    #[test]
    fn a_closed_terminal_never_stops_the_bell() {
        let fixture = BellFixture::ringing(ServiceFixture::new());
        let mut bell = Bell::new(Box::new(CapturedWriter::broken_pipe()));
        let service = fixture.service();

        bell.watch(&[BellFixture::row(SessionState::Working, false)], &service)
            .unwrap();
        let rung = bell.watch(&[BellFixture::row(SessionState::Waiting, false)], &service);

        assert!(rung.is_ok());
    }
}

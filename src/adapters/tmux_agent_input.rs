use std::thread;
use std::time::{Duration, Instant};

use super::tmux_command::TmuxCommand;
use super::tmux_error::TmuxError;
use super::tmux_launch::TmuxLaunch;
use super::tmux_scripts::TmuxScripts;
use crate::application::ports::MultiplexerError;

pub struct TmuxAgentInput<'a> {
    tmux: &'a TmuxCommand,
    patience: Duration,
}

impl<'a> TmuxAgentInput<'a> {
    const CURSOR_LABEL: &'static str = "ringleader-cursor";
    const CURSOR_FORMAT: &'static str =
        "ringleader-cursor #{pane_dead} #{cursor_flag} #{cursor_x} #{cursor_y}";
    const POLL_INTERVAL: Duration = Duration::from_millis(100);

    pub fn new(tmux: &'a TmuxCommand, patience: Duration) -> Self {
        Self { tmux, patience }
    }

    pub fn type_when_ready(&self, pane: &str, lines: &[String]) -> Result<(), InputError> {
        self.wait_for_prompt(pane)?;
        for line in lines {
            self.type_line(pane, line)?;
        }
        Ok(())
    }

    fn wait_for_prompt(&self, pane: &str) -> Result<(), InputError> {
        let deadline = Instant::now() + self.patience;
        loop {
            let cursor = self.cursor(pane)?;
            if cursor.is_at_prompt() {
                return Ok(());
            }
            if cursor.pane_dead || Instant::now() >= deadline {
                return Err(InputError::NotReady);
            }
            thread::sleep(Self::POLL_INTERVAL);
        }
    }

    fn cursor(&self, pane: &str) -> Result<AgentCursor, TmuxError> {
        let output = self
            .tmux
            .query(&TmuxScripts::pane_value(pane, Self::CURSOR_FORMAT))?;
        output
            .first(Self::CURSOR_LABEL)
            .and_then(AgentCursor::parse)
            .ok_or_else(|| TmuxError::failed("display-message", &output))
    }

    fn type_line(&self, pane: &str, line: &str) -> Result<(), TmuxError> {
        let output = self.tmux.change(&TmuxScripts::type_line(pane, line))?;
        if output.has(TmuxLaunch::DONE_LABEL) {
            return Ok(());
        }
        Err(TmuxError::failed("send-keys", &output))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    NotReady,
    Tmux(TmuxError),
}

impl InputError {
    pub fn in_session(self, tmux_name: &str) -> MultiplexerError {
        match self {
            InputError::NotReady => MultiplexerError::AgentNotReady {
                tmux_name: tmux_name.to_string(),
            },
            InputError::Tmux(error) => error.into(),
        }
    }
}

impl From<TmuxError> for InputError {
    fn from(error: TmuxError) -> Self {
        InputError::Tmux(error)
    }
}

struct AgentCursor {
    pane_dead: bool,
    shown: bool,
    left_home: bool,
}

impl AgentCursor {
    const ON: &'static str = "1";
    const HOME: &'static str = "0";

    fn parse(values: &str) -> Option<Self> {
        let [pane_dead, shown, column, row] = values.split(' ').collect::<Vec<_>>()[..] else {
            return None;
        };
        Some(Self {
            pane_dead: pane_dead == Self::ON,
            shown: shown == Self::ON,
            left_home: column != Self::HOME || row != Self::HOME,
        })
    }

    fn is_at_prompt(&self) -> bool {
        !self.pane_dead && self.shown && self.left_home
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TmuxTestServer;

    struct InputFixture {
        server: TmuxTestServer,
    }

    impl InputFixture {
        const TARGET: &'static str = "agent:";
        const SHORT_PATIENCE: Duration = Duration::from_millis(300);
        const LONG_PATIENCE: Duration = Duration::from_secs(5);
        const AT_PROMPT: &'static str = "printf '> '; exec cat";
        const LATE_PROMPT: &'static str = "sleep 0.5; printf '> '; exec cat";
        const HIDDEN_CURSOR: &'static str = "printf '\\033[?25l> '; exec cat";
        const NOTHING_DRAWN: &'static str = "exec cat";
        const EXITING: &'static str = "sleep 0.3; printf '> '; exit 0";

        fn running(script: &str) -> Self {
            let server = TmuxTestServer::new();
            server.text(&["new-session", "-d", "-s", "agent", "sh", "-c", script]);
            server.text(&[
                "set-option",
                "-p",
                "-t",
                Self::TARGET,
                "remain-on-exit",
                "on",
            ]);
            Self { server }
        }

        fn type_lines(&self, lines: &[&str], patience: Duration) -> Result<(), InputError> {
            let tmux = self.server.command();
            let lines: Vec<String> = lines.iter().map(|line| line.to_string()).collect();
            let pane = self.server.pane_of(Self::TARGET);
            TmuxAgentInput::new(&tmux, patience).type_when_ready(&pane, &lines)
        }

        fn shows(&self, text: &str) -> bool {
            self.server.wait_for(|| self.screen().contains(text))
        }

        fn screen(&self) -> String {
            self.server
                .text(&["capture-pane", "-p", "-t", Self::TARGET])
        }

        fn wait_until_dead(&self) {
            let dead = self.server.wait_for(|| {
                self.server
                    .text(&["display-message", "-p", "-t", Self::TARGET, "#{pane_dead}"])
                    == "1"
            });
            assert!(dead, "the pane never exited");
        }
    }

    #[test]
    fn lines_reach_an_agent_at_its_prompt() {
        let fixture = InputFixture::running(InputFixture::AT_PROMPT);

        let typed = fixture.type_lines(&["hello demo"], InputFixture::LONG_PATIENCE);

        assert_eq!(typed, Ok(()));
        assert!(fixture.shows("hello demo"));
    }

    #[test]
    fn typing_waits_for_an_agent_that_reaches_its_prompt_late() {
        let fixture = InputFixture::running(InputFixture::LATE_PROMPT);

        let typed = fixture.type_lines(&["hello demo"], InputFixture::LONG_PATIENCE);

        assert_eq!(typed, Ok(()));
        assert!(fixture.shows("hello demo"));
    }

    #[test]
    fn each_line_is_typed_in_order() {
        let fixture = InputFixture::running(InputFixture::AT_PROMPT);

        fixture
            .type_lines(&["first line", "second line"], InputFixture::LONG_PATIENCE)
            .unwrap();

        assert!(fixture.shows("second line"));
        let screen = fixture.screen();
        let first = screen.find("first line").unwrap();
        assert!(first < screen.find("second line").unwrap());
    }

    #[test]
    fn tmux_syntax_in_a_line_arrives_verbatim() {
        let fixture = InputFixture::running(InputFixture::AT_PROMPT);

        fixture
            .type_lines(&["-x #{pane_id} ;"], InputFixture::LONG_PATIENCE)
            .unwrap();

        assert!(fixture.shows("-x #{pane_id} ;"));
    }

    #[test]
    fn an_agent_with_a_hidden_cursor_is_not_ready() {
        let fixture = InputFixture::running(InputFixture::HIDDEN_CURSOR);

        let typed = fixture.type_lines(&["hello demo"], InputFixture::SHORT_PATIENCE);

        assert_eq!(typed, Err(InputError::NotReady));
        assert!(!fixture.screen().contains("hello demo"));
    }

    #[test]
    fn an_agent_that_has_drawn_nothing_is_not_ready() {
        let fixture = InputFixture::running(InputFixture::NOTHING_DRAWN);

        let typed = fixture.type_lines(&["hello demo"], InputFixture::SHORT_PATIENCE);

        assert_eq!(typed, Err(InputError::NotReady));
        assert!(!fixture.screen().contains("hello demo"));
    }

    #[test]
    fn an_exited_agent_is_not_ready_without_waiting() {
        let fixture = InputFixture::running(InputFixture::EXITING);
        fixture.wait_until_dead();
        let started = Instant::now();

        let typed = fixture.type_lines(&["hello demo"], InputFixture::LONG_PATIENCE);

        assert_eq!(typed, Err(InputError::NotReady));
        assert!(started.elapsed() < InputFixture::LONG_PATIENCE);
    }
}

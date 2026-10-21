use std::thread;
use std::time::{Duration, Instant};

use super::agent_cursor::AgentCursor;
use super::tmux_command::TmuxCommand;
use super::tmux_error::TmuxError;
use super::tmux_launch::TmuxLaunch;
use super::tmux_output::TmuxOutput;
use super::tmux_scripts::TmuxScripts;
use super::tmux_sequence::TmuxSequence;
use crate::application::ports::MultiplexerError;

pub struct TmuxAgentInput<'a> {
    tmux: &'a TmuxCommand,
    patience: Duration,
}

impl<'a> TmuxAgentInput<'a> {
    const CURSOR_LABEL: &'static str = "ringleader-cursor";
    const CURSOR_FORMAT: &'static str =
        "ringleader-cursor #{pane_dead} #{pane_in_mode} #{cursor_flag} #{cursor_x} #{cursor_y}";
    const POLL_INTERVAL: Duration = Duration::from_millis(100);
    const LINE_SETTLE: Duration = Duration::from_millis(300);
    const SUBMIT_PAUSE: Duration = Duration::from_millis(300);
    const NEXT_LINE_PATIENCE: Duration = Duration::from_secs(2);
    const PROMPT_BUFFER_PREFIX: &'static str = "ringleader-prompt-";

    pub fn new(tmux: &'a TmuxCommand, patience: Duration) -> Self {
        Self { tmux, patience }
    }

    pub fn type_when_ready(&self, pane: &str, lines: &[String]) -> Result<(), InputError> {
        let Some((first, rest)) = lines.split_first() else {
            return Ok(());
        };
        self.type_at_prompt(pane, first, self.patience)?;
        for line in rest {
            thread::sleep(Self::LINE_SETTLE);
            self.type_at_prompt(pane, line, Self::NEXT_LINE_PATIENCE)?;
        }
        Ok(())
    }

    pub fn paste_when_ready(&self, pane: &str, text: &str) -> Result<(), InputError> {
        self.wait_for_prompt(pane, self.patience)?;
        self.paste(pane, text)?;
        Ok(self.submit(pane)?)
    }

    pub fn is_ready(&self, pane: &str) -> Result<bool, TmuxError> {
        let cursor = self.cursor(pane)?;
        self.is_at_prompt(pane, &cursor)
    }

    fn type_at_prompt(&self, pane: &str, line: &str, patience: Duration) -> Result<(), InputError> {
        self.wait_for_prompt(pane, patience)?;
        Ok(self.type_line(pane, line)?)
    }

    fn wait_for_prompt(&self, pane: &str, patience: Duration) -> Result<(), InputError> {
        let deadline = Instant::now() + patience;
        loop {
            let cursor = self.cursor(pane)?;
            if self.is_at_prompt(pane, &cursor)? {
                return Ok(());
            }
            if cursor.is_dead() || Instant::now() >= deadline {
                return Err(InputError::NotReady);
            }
            thread::sleep(Self::POLL_INTERVAL);
        }
    }

    fn is_at_prompt(&self, pane: &str, cursor: &AgentCursor) -> Result<bool, TmuxError> {
        if !cursor.takes_keys() {
            return Ok(false);
        }
        let row = self.cursor_row(pane, cursor)?;
        Ok(cursor.follows_a_bare_prompt_marker(&row))
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

    fn cursor_row(&self, pane: &str, cursor: &AgentCursor) -> Result<String, TmuxError> {
        let output = self
            .tmux
            .query(&TmuxScripts::capture_row(pane, cursor.row()))?;
        if !output.succeeded() {
            return Err(TmuxError::failed("capture-pane", &output));
        }
        let row = output.stdout().lines().next().unwrap_or_default();
        Ok(row.to_string())
    }

    fn type_line(&self, pane: &str, line: &str) -> Result<(), TmuxError> {
        self.send(&TmuxScripts::type_text(pane, line))?;
        self.submit(pane)
    }

    fn paste(&self, pane: &str, text: &str) -> Result<(), TmuxError> {
        let buffer = format!("{}{}", Self::PROMPT_BUFFER_PREFIX, std::process::id());
        let paste = TmuxScripts::paste_from_input(pane, &buffer);
        let output = self.tmux.change_with_input(&paste, text.as_bytes())?;
        Self::confirmed(&output, "paste-buffer")
    }

    fn submit(&self, pane: &str) -> Result<(), TmuxError> {
        thread::sleep(Self::SUBMIT_PAUSE);
        self.send(&TmuxScripts::press_enter(pane))
    }

    fn send(&self, keys: &TmuxSequence) -> Result<(), TmuxError> {
        let output = self.tmux.change(keys)?;
        Self::confirmed(&output, "send-keys")
    }

    fn confirmed(output: &TmuxOutput, subcommand: &str) -> Result<(), TmuxError> {
        if output.has(TmuxLaunch::DONE_LABEL) {
            return Ok(());
        }
        Err(TmuxError::failed(subcommand, output))
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
        const PASTE_AWARE: &'static str = "stty raw -echo; printf '> '; while :; do \
            read_bytes=$(dd bs=4096 count=1 2>/dev/null | od -An -tx1 | tr -d ' \\n'); \
            [ \"$read_bytes\" = 0d ] && printf '[submitted]'; done";
        const BRACKETED_PASTE_AWARE: &'static str = "stty raw -echo; printf '\\033[?2004h> '; while :; do \
            read_bytes=$(dd bs=4096 count=1 2>/dev/null | od -An -tx1 | tr -d ' \\n'); \
            printf '[%s]' \"$read_bytes\"; done";
        const BRACKETED_TWO_LINES: &'static str = "[1b5b3230307e68690a796f1b5b3230317e]";
        const LONE_ENTER: &'static str = "[0d]";
        const REDRAWS_PROMPT: &'static str =
            "printf '> '; while read -r line; do printf 'got %s\\n> ' \"$line\"; done";
        const DIALOG_AFTER_A_LINE: &'static str =
            "printf '> '; read -r line; printf '\\033[?25l[dialog] '; exec cat";
        const PLACEHOLDER: &'static str = "printf '> Try this\\r\\033[2C'; exec cat";
        const DRAFT: &'static str = "printf '> delete the old'; exec cat";
        const SYMBOL_DRAFT: &'static str = "printf '> /'; exec cat";
        const WIDE_DRAFT: &'static str = "printf '> 👍'; exec cat";
        const WIDE_MARKER: &'static str = "printf '🤖 Ask anything\\r\\033[3C'; exec cat";
        const DRAFT_SECOND_LINE: &'static str = "printf '> first line\\n  '; exec cat";
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

        fn paste(&self, text: &str, patience: Duration) -> Result<(), InputError> {
            let tmux = self.server.command();
            let pane = self.server.pane_of(Self::TARGET);
            TmuxAgentInput::new(&tmux, patience).paste_when_ready(&pane, text)
        }

        fn buffers(&self) -> String {
            self.server.text(&["list-buffers"])
        }

        fn enter_copy_mode(&self) {
            self.server.text(&["copy-mode", "-t", Self::TARGET]);
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
    fn a_line_is_submitted_by_an_enter_that_arrives_on_its_own() {
        let fixture = InputFixture::running(InputFixture::PASTE_AWARE);

        let typed = fixture.type_lines(
            &["a line long enough that an agent could read it as a paste"],
            InputFixture::LONG_PATIENCE,
        );

        assert_eq!(typed, Ok(()));
        assert!(fixture.shows("[submitted]"));
    }

    #[test]
    fn a_pasted_prompt_arrives_as_one_bracketed_paste_that_keeps_its_line_breaks() {
        let fixture = InputFixture::running(InputFixture::BRACKETED_PASTE_AWARE);

        let pasted = fixture.paste("hi\nyo", InputFixture::LONG_PATIENCE);

        assert_eq!(pasted, Ok(()));
        assert!(fixture.shows(InputFixture::BRACKETED_TWO_LINES));
    }

    #[test]
    fn a_pasted_prompt_is_submitted_by_an_enter_that_arrives_on_its_own() {
        let fixture = InputFixture::running(InputFixture::BRACKETED_PASTE_AWARE);

        fixture
            .paste("hi\nyo", InputFixture::LONG_PATIENCE)
            .unwrap();

        assert!(fixture.shows(InputFixture::LONE_ENTER));
    }

    #[test]
    fn tmux_syntax_in_a_pasted_prompt_arrives_verbatim() {
        let fixture = InputFixture::running(InputFixture::AT_PROMPT);

        fixture
            .paste("-x #{pane_id} ;", InputFixture::LONG_PATIENCE)
            .unwrap();

        assert!(fixture.shows("-x #{pane_id} ;"));
    }

    #[test]
    fn a_paste_leaves_no_tmux_buffer_behind() {
        let fixture = InputFixture::running(InputFixture::AT_PROMPT);

        fixture
            .paste("hello demo", InputFixture::LONG_PATIENCE)
            .unwrap();

        assert_eq!(fixture.buffers(), "");
    }

    #[test]
    fn a_draft_keeps_a_prompt_from_being_pasted() {
        let fixture = InputFixture::running(InputFixture::DRAFT);

        let pasted = fixture.paste("hello demo", InputFixture::SHORT_PATIENCE);

        assert_eq!(pasted, Err(InputError::NotReady));
        assert!(!fixture.screen().contains("hello demo"));
    }

    #[test]
    fn placeholder_text_after_the_cursor_leaves_the_prompt_ready() {
        let fixture = InputFixture::running(InputFixture::PLACEHOLDER);

        let typed = fixture.type_lines(&["hello demo"], InputFixture::LONG_PATIENCE);

        assert_eq!(typed, Ok(()));
        assert!(fixture.shows("hello demo"));
    }

    #[test]
    fn a_wide_prompt_marker_leaves_the_prompt_ready() {
        let fixture = InputFixture::running(InputFixture::WIDE_MARKER);

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
        let fixture = InputFixture::running(InputFixture::REDRAWS_PROMPT);

        fixture
            .type_lines(&["first line", "second line"], InputFixture::LONG_PATIENCE)
            .unwrap();

        assert!(fixture.shows("second line"));
        let screen = fixture.screen();
        let first = screen.find("first line").unwrap();
        assert!(first < screen.find("second line").unwrap());
    }

    #[test]
    fn a_line_is_not_typed_into_a_dialog_the_previous_line_opened() {
        let fixture = InputFixture::running(InputFixture::DIALOG_AFTER_A_LINE);

        let typed = fixture.type_lines(&["/first", "second"], InputFixture::LONG_PATIENCE);

        assert_eq!(typed, Err(InputError::NotReady));
        assert!(fixture.shows("[dialog]"));
        assert!(!fixture.screen().contains("second"));
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
    fn a_pane_in_copy_mode_is_not_ready() {
        let fixture = InputFixture::running(InputFixture::AT_PROMPT);
        fixture.enter_copy_mode();

        let typed = fixture.type_lines(&["hello demo"], InputFixture::SHORT_PATIENCE);

        assert_eq!(typed, Err(InputError::NotReady));
        assert!(!fixture.screen().contains("hello demo"));
    }

    #[test]
    fn an_unsent_draft_keeps_the_agent_from_being_ready() {
        let fixture = InputFixture::running(InputFixture::DRAFT);

        let typed = fixture.type_lines(&["hello demo"], InputFixture::SHORT_PATIENCE);

        assert_eq!(typed, Err(InputError::NotReady));
        assert!(!fixture.screen().contains("hello demo"));
    }

    #[test]
    fn a_draft_of_symbols_keeps_the_agent_from_being_ready() {
        let fixture = InputFixture::running(InputFixture::SYMBOL_DRAFT);

        let typed = fixture.type_lines(&["hello demo"], InputFixture::SHORT_PATIENCE);

        assert_eq!(typed, Err(InputError::NotReady));
        assert!(!fixture.screen().contains("hello demo"));
    }

    #[test]
    fn a_draft_of_wide_characters_keeps_the_agent_from_being_ready() {
        let fixture = InputFixture::running(InputFixture::WIDE_DRAFT);

        let typed = fixture.type_lines(&["hello demo"], InputFixture::SHORT_PATIENCE);

        assert_eq!(typed, Err(InputError::NotReady));
        assert!(!fixture.screen().contains("hello demo"));
    }

    #[test]
    fn a_cursor_on_the_second_line_of_a_draft_is_not_ready() {
        let fixture = InputFixture::running(InputFixture::DRAFT_SECOND_LINE);

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

use std::cell::Cell;
use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::Line;

use super::dialogs::DialogStep;
use super::text_fit::TextFit;
use super::theme::Theme;

pub struct HelpView {
    sections: Vec<(&'static str, Vec<String>)>,
    scroll: usize,
    max_scroll: Cell<usize>,
}

impl HelpView {
    pub const TITLE: &'static str = " Help ";
    const KEYS: [(&'static str, &'static str); 10] = [
        ("n", "create a session"),
        ("d", "delete the selected session"),
        ("R", "rename the selected session and its branch"),
        ("Enter", "attach to the selected session"),
        ("t", "open a shell in the selected session's worktree"),
        ("s", "restart an exited or stopped session"),
        ("r", "reload the config and the registry and refresh diffs"),
        ("↑ ↓ j k h l", "move the selection"),
        ("?", "show or close this help"),
        ("q", "quit, the sessions keep running"),
    ];
    const KEY_WIDTH: usize = 12;
    const CLOSING: &'static str = "↑ ↓ scroll · ? Esc q close";

    pub fn new(config_path: &Path, log_path: &Path, tmux_prefix: &str) -> Self {
        let keys = Self::KEYS
            .iter()
            .map(|(key, action)| format!("{key:<width$}{action}", width = Self::KEY_WIDTH))
            .collect();
        let sections = vec![
            ("Keys", keys),
            ("Returning with Ctrl-Q", Self::return_key(tmux_prefix)),
            ("Bell", Self::bell()),
            ("Files", Self::files(config_path, log_path)),
        ];
        Self {
            sections,
            scroll: 0,
            max_scroll: Cell::new(0),
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> DialogStep {
        match key.code {
            KeyCode::Char('?' | 'q') | KeyCode::Esc => return DialogStep::Close,
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll = (self.visible_scroll() + 1).min(self.max_scroll.get());
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll = self.visible_scroll().saturating_sub(1)
            }
            _ => {}
        }
        DialogStep::Stay
    }

    pub fn lines(&self, width: usize, height: usize, theme: &Theme) -> Vec<Line<'static>> {
        let mut content = Vec::new();
        for (heading, paragraphs) in &self.sections {
            content.push(Line::styled(*heading, theme.emphasis()));
            for paragraph in paragraphs {
                content.extend(TextFit::new(paragraph).styled_lines(width, theme.base()));
            }
            content.push(Line::from(""));
        }
        let room = height.saturating_sub(1);
        self.max_scroll.set(content.len().saturating_sub(room));
        let shown = content.into_iter().skip(self.visible_scroll()).take(room);
        let mut lines: Vec<Line<'static>> = shown.collect();
        lines.push(Line::styled(Self::CLOSING, theme.dim()));
        lines
    }

    fn visible_scroll(&self) -> usize {
        self.scroll.min(self.max_scroll.get())
    }

    fn return_key(tmux_prefix: &str) -> Vec<String> {
        vec![
            "Ctrl-Q returns here from a managed session. Outside tmux it detaches the client, and inside tmux it switches the client back to this pane.".to_string(),
            format!("It works only in sessions whose name starts with the current tmux_prefix {tmux_prefix}. Everywhere else Ctrl-Q reaches the pane."),
            "A terminal emulator started in the foreground of a tmux pane counts as inside tmux. Run env -u TMUX -u TMUX_PANE ringleader to force outside mode against the default server.".to_string(),
            "Open extra shells with t instead of splitting the agent window, since output in a split pane counts as agent activity.".to_string(),
        ]
    }

    fn bell() -> Vec<String> {
        vec![
            "With bell_on_waiting on, the bell rings when an agent goes from working to waiting, unless a client shows its agent window. An exit never rings.".to_string(),
            "While an attach from outside tmux runs, this screen is suspended and does not poll, so the bell rings only inside tmux or while this screen shows.".to_string(),
        ]
    }

    fn files(config_path: &Path, log_path: &Path) -> Vec<String> {
        vec![
            format!("Config   {}", config_path.display()),
            format!("Log      {}", log_path.display()),
            format!("Version  ringleader {}", env!("CARGO_PKG_VERSION")),
        ]
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::ui::dialogs::Dialog;

    struct HelpFixture;

    impl HelpFixture {
        fn help() -> HelpView {
            HelpView::new(
                Path::new("/config/ringleader/config.toml"),
                Path::new("/state/ringleader/app.log"),
                "ringleader-",
            )
        }

        fn screen(help: HelpView, width: u16, height: u16) -> String {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| Dialog::Help(Box::new(help)).render(frame, &Theme))
                .unwrap();
            let buffer = terminal.backend().buffer();
            buffer
                .content()
                .chunks(usize::from(width))
                .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        }

        fn text(help: &HelpView) -> String {
            help.lines(70, 200, &Theme)
                .iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join(" ")
        }

        const SHORT_WIDTH: usize = 56;
        const SHORT_HEIGHT: usize = 8;
        const PAST_THE_END: usize = 50;

        fn press(help: &mut HelpView, code: KeyCode) -> DialogStep {
            help.handle_key(&KeyEvent::new(code, KeyModifiers::NONE))
        }

        fn press_times(help: &mut HelpView, code: KeyCode, times: usize) {
            for _ in 0..times {
                Self::press(help, code);
            }
        }

        fn short_page(help: &HelpView) -> Vec<Line<'static>> {
            help.lines(Self::SHORT_WIDTH, Self::SHORT_HEIGHT, &Theme)
        }

        fn scrolled_past_the_end() -> HelpView {
            let mut help = Self::help();
            Self::short_page(&help);
            Self::press_times(&mut help, KeyCode::Down, Self::PAST_THE_END);
            help
        }
    }

    #[test]
    fn the_help_overlay_is_centered_and_lists_the_keys() {
        let screen = HelpFixture::screen(HelpFixture::help(), 100, 28);

        assert!(screen.lines().nth(3).unwrap().contains("┌ Help "));
        assert!(screen.contains("n           create a session"));
        assert!(screen.contains("q           quit, the sessions keep running"));
    }

    #[test]
    fn the_help_names_the_config_and_log_paths_and_the_version() {
        let text = HelpFixture::text(&HelpFixture::help());

        assert!(text.contains("Config   /config/ringleader/config.toml"));
        assert!(text.contains("Log      /state/ringleader/app.log"));
        assert!(text.contains(&format!(
            "Version  ringleader {}",
            env!("CARGO_PKG_VERSION")
        )));
    }

    #[test]
    fn the_help_explains_ctrl_q_and_the_inside_tmux_caveat() {
        let text = HelpFixture::text(&HelpFixture::help());

        assert!(text.contains("current tmux_prefix ringleader-"));
        assert!(text.contains(
            "A terminal emulator started in the foreground of a tmux pane counts as inside tmux."
        ));
        assert!(text.contains("env -u TMUX -u TMUX_PANE ringleader"));
        assert!(text.contains("Open extra shells with t instead of splitting the agent window"));
        assert!(text.contains("the bell rings only inside tmux or while this screen shows"));
    }

    #[test]
    fn question_mark_esc_and_q_close_the_help() {
        let mut help = HelpFixture::help();

        for code in [KeyCode::Char('?'), KeyCode::Esc, KeyCode::Char('q')] {
            assert_eq!(HelpFixture::press(&mut help, code), DialogStep::Close);
        }
    }

    #[test]
    fn a_short_terminal_scrolls_the_help() {
        let mut help = HelpFixture::help();
        let top = help.lines(56, 8, &Theme);

        HelpFixture::press(&mut help, KeyCode::Down);
        let scrolled = help.lines(56, 8, &Theme);

        assert_eq!(top[0].spans[0].content, "Keys");
        assert_eq!(scrolled[0], top[1]);
    }

    #[test]
    fn up_moves_at_once_after_scrolling_past_the_end() {
        let mut help = HelpFixture::scrolled_past_the_end();
        let last_page = HelpFixture::short_page(&help);

        HelpFixture::press(&mut help, KeyCode::Up);
        let moved = HelpFixture::short_page(&help);

        assert_eq!(moved[1], last_page[0]);
    }

    #[test]
    fn down_at_the_end_keeps_the_last_page() {
        let mut help = HelpFixture::scrolled_past_the_end();
        let last_page = HelpFixture::short_page(&help);

        HelpFixture::press(&mut help, KeyCode::Down);

        assert_eq!(HelpFixture::short_page(&help), last_page);
    }

    #[test]
    fn a_taller_draw_leaves_no_hidden_offset_for_up() {
        let mut help = HelpFixture::scrolled_past_the_end();
        HelpFixture::text(&help);

        HelpFixture::press(&mut help, KeyCode::Up);

        assert_eq!(HelpFixture::short_page(&help)[0].spans[0].content, "Keys");
    }
}

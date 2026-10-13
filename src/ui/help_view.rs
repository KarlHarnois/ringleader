use std::cell::Cell;
use std::iter::once;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::Line;

use super::dialogs::DialogStep;
use super::dialogs::home_path::HomePath;
use super::legend::{Legend, LegendEntry};
use super::session_table::StateIndicator;
use super::text_fit::TextFit;
use super::theme::Theme;
use crate::domain::{ReturnKey, SessionState};

pub struct HelpView {
    return_key: String,
    files: Vec<(String, PathBuf)>,
    home: HomePath,
    scroll: usize,
    max_scroll: Cell<usize>,
}

impl HelpView {
    pub const TITLE: &'static str = concat!(" Help · ringleader ", env!("CARGO_PKG_VERSION"), " ");
    const KEYS_HEADING: &'static str = "Keys";
    const KEYS: [(&'static str, &'static str); 9] = [
        ("n", "create a session"),
        ("d", "delete the selected session"),
        ("R", "rename the selected session"),
        ("Enter", "attach to the selected session"),
        ("t", "open a shell in the selected session's worktree"),
        ("s", "restart an exited or stopped session"),
        ("r", "reload the config and the registry and refresh diffs"),
        ("↑ ↓ j k", "move the selection"),
        ("q", "quit, the sessions keep running"),
    ];
    const RETURN_ACTION: &'static str = "detach from a session and return here";
    const STATES: [SessionState; 5] = [
        SessionState::Working,
        SessionState::Waiting,
        SessionState::Exited,
        SessionState::Stopped,
        SessionState::Unmanaged,
    ];
    const FIRST_SPINNER_TICK: u64 = 0;
    const LABEL_WIDTH: usize = 13;
    const CLOSING: &'static str = "↑ ↓ scroll · ? Esc q close";

    pub fn new(return_key: &ReturnKey, files: &[(&str, &Path)], home: HomePath) -> Self {
        let files = files
            .iter()
            .map(|(label, path)| (label.to_string(), path.to_path_buf()))
            .collect();
        Self {
            return_key: return_key.display(),
            files,
            home,
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
        let content = self.content(width, theme);
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

    fn content(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let sections = [
            self.keys(width, theme),
            Self::legends(width, theme),
            self.files(width, theme),
        ];
        sections.join(&Line::default())
    }

    fn keys(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let heading = Line::styled(Self::KEYS_HEADING, theme.emphasis());
        let rows = Self::KEYS
            .iter()
            .flat_map(|(key, action)| Self::labeled(key, action, width, theme));
        let return_row = Self::labeled(&self.return_key, Self::RETURN_ACTION, width, theme);
        once(heading).chain(rows).chain(return_row).collect()
    }

    fn legends(width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let legends = [Self::state_legend(theme), Self::diff_legend(theme)];
        legends
            .iter()
            .flat_map(|legend| legend.lines(width, theme))
            .collect()
    }

    fn state_legend(theme: &Theme) -> Legend {
        let entries = Self::STATES.iter().map(|&state| {
            let indicator = StateIndicator::new(state, Self::FIRST_SPINNER_TICK);
            LegendEntry::new(indicator.glyph(), indicator.style(theme), state.label())
        });
        Legend::new(Self::label_column("States"), entries.collect())
    }

    fn diff_legend(theme: &Theme) -> Legend {
        let entries = vec![
            LegendEntry::new("+n", theme.insertions(), "added lines"),
            LegendEntry::new("-n", theme.deletions(), "removed lines"),
            LegendEntry::new("?n", theme.base(), "untracked files"),
        ];
        Legend::new(Self::label_column("Diff"), entries)
    }

    fn files(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        self.files
            .iter()
            .flat_map(|(label, path)| Self::labeled(label, &self.home.shown(path), width, theme))
            .collect()
    }

    fn labeled(label: &str, text: &str, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let row = format!("{}{text}", Self::label_column(label));
        TextFit::new(&row).styled_lines(width, theme.base())
    }

    fn label_column(label: &str) -> String {
        TextFit::new(label).padded(Self::LABEL_WIDTH)
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Style;

    use super::*;
    use crate::ui::dialogs::Dialog;

    struct HelpFixture;

    impl HelpFixture {
        const HOME: &'static str = "/home/example";
        const FULL_WIDTH: u16 = 80;
        const FULL_HEIGHT: u16 = 24;
        const SHORT_WIDTH: usize = 56;
        const SHORT_HEIGHT: usize = 8;
        const PAST_THE_END: usize = 50;

        fn help() -> HelpView {
            Self::help_for(&ReturnKey::default())
        }

        fn help_for(return_key: &ReturnKey) -> HelpView {
            HelpView::new(
                return_key,
                &[
                    (
                        "Config",
                        Path::new("/home/example/.config/ringleader/config.toml"),
                    ),
                    (
                        "Log",
                        Path::new("/home/example/.local/state/ringleader/app.log"),
                    ),
                    (
                        "tmux socket",
                        Path::new("/run/user/1000/ringleader/tmux/tmux.sock"),
                    ),
                    (
                        "tmux config",
                        Path::new("/home/example/.local/state/ringleader/tmux.conf"),
                    ),
                ],
                HomePath::new(Some(Path::new(Self::HOME))),
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

        fn rows(help: &HelpView, width: usize) -> Vec<String> {
            help.lines(width, 200, &Theme)
                .iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<String>()
                })
                .collect()
        }

        fn text(help: &HelpView) -> String {
            Self::rows(help, 70).join(" ")
        }

        fn span_style(symbol: &str) -> Style {
            let lines = Self::help().lines(70, 200, &Theme);
            let span = lines
                .iter()
                .flat_map(|line| line.spans.iter())
                .find(|span| span.content == symbol)
                .unwrap();
            span.style
        }

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
        assert!(screen.contains("n            create a session"));
        assert!(screen.contains("q            quit, the sessions keep running"));
    }

    #[test]
    fn the_whole_help_fits_the_smallest_full_size_dialog() {
        let screen = HelpFixture::screen(
            HelpFixture::help(),
            HelpFixture::FULL_WIDTH,
            HelpFixture::FULL_HEIGHT,
        );

        assert!(screen.contains("Keys"));
        assert!(screen.contains("tmux config  ~/.local/state/ringleader/tmux.conf"));
    }

    #[test]
    fn the_title_names_the_version() {
        let screen = HelpFixture::screen(HelpFixture::help(), 100, 28);

        assert!(screen.contains(&format!(
            "┌ Help · ringleader {} ",
            env!("CARGO_PKG_VERSION")
        )));
    }

    #[test]
    fn the_help_lists_the_configured_return_key() {
        let text = HelpFixture::text(&HelpFixture::help_for(&ReturnKey::parse("M-x").unwrap()));

        assert!(text.contains("Alt-x        detach from a session and return here"));
    }

    #[test]
    fn the_help_shows_the_files_under_home_from_home() {
        let text = HelpFixture::text(&HelpFixture::help());

        assert!(text.contains("Config       ~/.config/ringleader/config.toml"));
        assert!(text.contains("Log          ~/.local/state/ringleader/app.log"));
        assert!(text.contains("tmux config  ~/.local/state/ringleader/tmux.conf"));
    }

    #[test]
    fn the_help_shows_the_tmux_socket_outside_home_in_full() {
        let text = HelpFixture::text(&HelpFixture::help());

        assert!(text.contains("tmux socket  /run/user/1000/ringleader/tmux/tmux.sock"));
    }

    #[test]
    fn the_legend_names_the_glyph_of_every_state() {
        let text = HelpFixture::text(&HelpFixture::help());

        assert!(
            text.contains("States       ⠋ working  ● waiting  ✗ exited  ○ stopped  ? unmanaged")
        );
    }

    #[test]
    fn the_legend_colors_each_glyph_like_the_session_table() {
        assert_eq!(
            HelpFixture::span_style("●"),
            Theme.state(SessionState::Waiting)
        );
    }

    #[test]
    fn the_legend_explains_the_diff_column() {
        let text = HelpFixture::text(&HelpFixture::help());

        assert!(text.contains("Diff         +n added lines  -n removed lines  ?n untracked files"));
    }

    #[test]
    fn the_legend_colors_the_diff_markers_like_the_session_table() {
        assert_eq!(HelpFixture::span_style("+n"), Theme.insertions());
        assert_eq!(HelpFixture::span_style("-n"), Theme.deletions());
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

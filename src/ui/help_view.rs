use std::cell::Cell;
use std::iter::once;
use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::{Line, Span};

use super::dialogs::DialogStep;
use super::dialogs::home_path::HomePath;
use super::label_column::LabelColumn;
use super::legend::{Legend, LegendEntry};
use super::state_indicator::StateIndicator;
use super::text_fit::TextFit;
use super::theme::Theme;
use crate::domain::{PullRequestState, ReturnKey, SessionState};

pub struct HelpView {
    return_key: String,
    files: Vec<(String, String)>,
    scroll: usize,
    max_scroll: Cell<usize>,
}

impl HelpView {
    pub const TITLE: &'static str = concat!(" Help · ringleader ", env!("CARGO_PKG_VERSION"), " ");
    const KEYS_HEADING: &'static str = "Keys";
    const KEYS: [(&'static str, &'static str); 13] = [
        ("n", "create a session"),
        ("d", "delete the selected session"),
        ("R", "rename the selected session and its branch"),
        (
            "Enter",
            "attach to the selected session, or fold the selected repo",
        ),
        ("Space", "fold or unfold the selected repo"),
        ("/ Ctrl-K", "find a session by name and attach to it"),
        ("t", "open a shell in the selected session's worktree"),
        ("s", "restart an exited or stopped session"),
        (
            "r",
            "reload the config and the registry and refresh diffs and PRs",
        ),
        ("↑ ↓ j k h l", "move the selection"),
        ("gg G", "select the first or the last row"),
        ("1-9", "jump to a repo"),
        ("q", "quit, the sessions keep running"),
    ];
    const RETURN_ACTION: &'static str = "detach from a session and return here";
    const FIRST_SPINNER_TICK: u64 = 0;
    const PULL_REQUEST_SYMBOL: &'static str = "#n";
    const CLOSE_HINT: &'static str = "? Esc q close";
    const SCROLL_HINT: &'static str = "↑ ↓ scroll · ";

    pub fn new(return_key: &ReturnKey, files: &[(&str, &Path)], home: &HomePath) -> Self {
        let files = files
            .iter()
            .map(|(label, path)| (label.to_string(), home.shown(path)))
            .collect();
        Self {
            return_key: return_key.display(),
            files,
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
        lines.push(Line::styled(self.hint(), theme.dim()));
        lines
    }

    fn hint(&self) -> String {
        if self.scrolls() {
            format!("{}{}", Self::SCROLL_HINT, Self::CLOSE_HINT)
        } else {
            Self::CLOSE_HINT.to_string()
        }
    }

    fn scrolls(&self) -> bool {
        self.max_scroll.get() > 0
    }

    fn visible_scroll(&self) -> usize {
        self.scroll.min(self.max_scroll.get())
    }

    fn content(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let mut content = self.keys(width, theme);
        content.extend(Self::legends(width, theme));
        content.extend(self.files(width, theme));
        content
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
        let legends = [
            Self::state_legend(theme),
            Self::pull_request_legend(theme),
            Self::diff_legend(theme),
        ];
        legends
            .iter()
            .flat_map(|legend| legend.lines(width, theme))
            .collect()
    }

    fn state_legend(theme: &Theme) -> Legend {
        let entries = SessionState::ALL.iter().map(|&state| {
            let indicator = StateIndicator::new(state, Self::FIRST_SPINNER_TICK);
            LegendEntry::new(indicator.glyph(), indicator.style(theme), state.label())
        });
        Legend::new("States", entries.collect())
    }

    fn pull_request_legend(theme: &Theme) -> Legend {
        let entries = PullRequestState::ALL.iter().map(|&state| {
            LegendEntry::new(
                Self::PULL_REQUEST_SYMBOL,
                theme.pull_request(state),
                state.label(),
            )
        });
        Legend::new("PRs", entries.collect())
    }

    fn diff_legend(theme: &Theme) -> Legend {
        let entries = vec![
            LegendEntry::new("+n", theme.insertions(), "added lines"),
            LegendEntry::new("-n", theme.deletions(), "removed lines"),
            LegendEntry::new("?n", theme.base(), "untracked files"),
        ];
        Legend::new("Diff", entries)
    }

    fn files(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        self.files
            .iter()
            .flat_map(|(label, shown_path)| Self::labeled(label, shown_path, width, theme))
            .collect()
    }

    fn labeled(label: &str, text: &str, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let column = LabelColumn::new(label);
        let wrapped = TextFit::new(text).wrapped(column.room(width));
        let rows = wrapped
            .into_iter()
            .map(|line| vec![Span::styled(line, theme.base())]);
        column.lines(rows, theme)
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
    use crate::ui::home_view::LayoutTier;

    struct HelpFixture;

    impl HelpFixture {
        const HOME: &'static str = "/home/example";
        const WIDE_WIDTH: usize = 70;
        const EVERY_ROW_HEIGHT: usize = 200;
        const SHORT_WIDTH: usize = 56;
        const SHORT_HEIGHT: usize = 8;
        const PAST_THE_END: usize = 50;
        const ROWS_ABOVE_THE_SMALLEST_FULL_SIZE: u16 = 3;
        const TALL_NARROW_SIZE: (u16, u16) = (79, 30);
        const TMUX_CONFIG_ROW: &'static str = "tmux config  ~/.local/state/ringleader/tmux.conf";

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
                &HomePath::new(Some(Path::new(Self::HOME))),
            )
        }

        fn smallest_size_that_fits() -> (u16, u16) {
            let (width, height) = LayoutTier::smallest_full_size();
            (width, height + Self::ROWS_ABOVE_THE_SMALLEST_FULL_SIZE)
        }

        fn screen(help: HelpView, width: u16, height: u16) -> String {
            Self::dialog_screen(&Dialog::Help(Box::new(help)), width, height)
        }

        fn screen_scrolled_to_the_end(width: u16, height: u16) -> String {
            let mut dialog = Dialog::Help(Box::new(Self::help()));
            Self::dialog_screen(&dialog, width, height);
            let Dialog::Help(help) = &mut dialog else {
                unreachable!()
            };
            Self::press_times(help, KeyCode::Down, Self::PAST_THE_END);
            Self::dialog_screen(&dialog, width, height)
        }

        fn dialog_screen(dialog: &Dialog, width: u16, height: u16) -> String {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| dialog.render(frame, &Theme)).unwrap();
            let buffer = terminal.backend().buffer();
            buffer
                .content()
                .chunks(usize::from(width))
                .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        }

        fn page(help: &HelpView, width: usize) -> Vec<Line<'static>> {
            help.lines(width, Self::EVERY_ROW_HEIGHT, &Theme)
        }

        fn rows(help: &HelpView, width: usize) -> Vec<String> {
            Self::page(help, width)
                .iter()
                .map(ToString::to_string)
                .collect()
        }

        fn text(help: &HelpView) -> String {
            Self::rows(help, Self::WIDE_WIDTH).join(" ")
        }

        fn span_style(symbol: &str) -> Style {
            let lines = Self::page(&Self::help(), Self::WIDE_WIDTH);
            let span = lines
                .iter()
                .flat_map(|line| line.spans.iter())
                .find(|span| span.content == symbol)
                .unwrap();
            span.style
        }

        fn pull_request_styles() -> Vec<Style> {
            let lines = Self::page(&Self::help(), Self::WIDE_WIDTH);
            let legend = lines
                .iter()
                .find(|line| line.to_string().starts_with("PRs"))
                .unwrap();
            legend
                .spans
                .iter()
                .filter(|span| span.content == HelpView::PULL_REQUEST_SYMBOL)
                .map(|span| span.style)
                .collect()
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

        assert!(screen.lines().nth(1).unwrap().contains("┌ Help "));
        assert!(screen.contains("n            create a session"));
        assert!(screen.contains("q            quit, the sessions keep running"));
    }

    #[test]
    fn a_wrapped_key_row_continues_under_its_action() {
        let rows = HelpFixture::rows(&HelpFixture::help(), HelpFixture::SHORT_WIDTH);

        assert!(rows.contains(&"t            open a shell in the selected session's".to_string()));
        assert!(rows.contains(&"             worktree".to_string()));
    }

    #[test]
    fn the_smallest_full_size_dialog_scrolls_down_to_the_last_file() {
        let (width, height) = LayoutTier::smallest_full_size();

        let first_page = HelpFixture::screen(HelpFixture::help(), width, height);
        let last_page = HelpFixture::screen_scrolled_to_the_end(width, height);

        assert!(!first_page.contains(HelpFixture::TMUX_CONFIG_ROW));
        assert!(last_page.contains(HelpFixture::TMUX_CONFIG_ROW));
    }

    #[test]
    fn a_tall_enough_full_size_terminal_shows_the_whole_help() {
        let (width, height) = HelpFixture::smallest_size_that_fits();

        let screen = HelpFixture::screen(HelpFixture::help(), width, height);

        assert!(screen.contains("Keys"));
        assert!(screen.contains(HelpFixture::TMUX_CONFIG_ROW));
        assert!(!screen.contains("scroll"));
    }

    #[test]
    fn a_help_that_fits_offers_no_scroll_keys() {
        let (width, height) = HelpFixture::TALL_NARROW_SIZE;

        let screen = HelpFixture::screen(HelpFixture::help(), width, height);

        assert!(screen.contains("? Esc q close"));
        assert!(!screen.contains("scroll"));
    }

    #[test]
    fn the_smallest_full_size_offers_the_scroll_keys() {
        let (width, height) = LayoutTier::smallest_full_size();

        let screen = HelpFixture::screen(HelpFixture::help(), width, height);

        assert!(screen.contains("↑ ↓ scroll · ? Esc q close"));
    }

    #[test]
    fn a_help_that_overflows_offers_the_scroll_keys() {
        let page = HelpFixture::short_page(&HelpFixture::help());

        assert_eq!(
            page.last().unwrap().spans[0].content,
            "↑ ↓ scroll · ? Esc q close"
        );
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
    fn the_help_lists_the_session_finder_keys() {
        let text = HelpFixture::text(&HelpFixture::help());

        assert!(text.contains("/ Ctrl-K     find a session by name and attach to it"));
    }

    #[test]
    fn a_long_return_key_shows_in_full_with_a_gap_before_its_action() {
        let long_key = ReturnKey::parse("C-M-S-F12").unwrap();

        let text = HelpFixture::text(&HelpFixture::help_for(&long_key));

        assert!(text.contains("Ctrl-Alt-Shift-F12  detach from a session and return here"));
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
    fn the_legend_names_every_pull_request_state() {
        let text = HelpFixture::text(&HelpFixture::help());

        assert!(text.contains("PRs          #n open  #n draft  #n merged  #n closed"));
    }

    #[test]
    fn the_legend_colors_each_pull_request_state_like_the_session_table() {
        assert_eq!(
            HelpFixture::pull_request_styles(),
            vec![
                Theme.pull_request(PullRequestState::Open),
                Theme.pull_request(PullRequestState::Draft),
                Theme.pull_request(PullRequestState::Merged),
                Theme.pull_request(PullRequestState::Closed),
            ]
        );
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
        let top = HelpFixture::short_page(&help);

        HelpFixture::press(&mut help, KeyCode::Down);
        let scrolled = HelpFixture::short_page(&help);

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

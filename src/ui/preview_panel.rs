use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Padding, Paragraph};

use super::home_view::CenteredText;
use super::theme::Theme;
use crate::domain::SessionSummary;

pub struct PreviewPanel<'a> {
    row: Option<&'a SessionSummary>,
    capture: Option<&'a PreviewCapture>,
    theme: Theme,
}

impl<'a> PreviewPanel<'a> {
    const SEPARATOR: char = '─';
    const ELLIPSIS: char = '…';
    const UNKNOWN_AGENT: &'static str = "unknown agent";

    pub fn new(
        row: Option<&'a SessionSummary>,
        capture: Option<&'a PreviewCapture>,
        theme: Theme,
    ) -> Self {
        Self {
            row,
            capture,
            theme,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Preview ")
            .padding(Padding::horizontal(1));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        match self.row {
            Some(row) => self.render_row(row, frame, inner),
            None => CenteredText::new(&CenteredText::EMPTY_STATE).render(frame, inner),
        }
    }

    fn render_row(&self, row: &SessionSummary, frame: &mut Frame, area: Rect) {
        let width = usize::from(area.width);
        let mut lines = vec![
            Line::styled(Self::title(row), self.theme.emphasis()),
            Line::styled(
                Self::left_truncated(&Self::worktree(row), width),
                self.theme.dim(),
            ),
            Line::styled(Self::SEPARATOR.to_string().repeat(width), self.theme.dim()),
        ];
        let room = usize::from(area.height).saturating_sub(lines.len());
        lines.extend(self.body(row, room).into_iter().map(Line::from));
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn body(&self, row: &SessionSummary, room: usize) -> Vec<String> {
        match self.capture {
            Some(PreviewCapture::Text(text)) => PreviewText::new(text).bottom_rows(room),
            Some(PreviewCapture::Unavailable(reason)) => Self::metadata_lines(row, reason),
            None => Vec::new(),
        }
    }

    fn metadata_lines(row: &SessionSummary, reason: &str) -> Vec<String> {
        let mut lines = vec![format!("No preview: {reason}")];
        if let Some(metadata) = row.metadata() {
            lines.push(format!("Repo: {}", metadata.repo_path().display()));
            lines.push(format!("Branch: {}", metadata.branch()));
            lines.push(format!("Base: {}", metadata.base_commit()));
        }
        lines
    }

    fn title(row: &SessionSummary) -> String {
        let agent = row
            .metadata()
            .map_or(Self::UNKNOWN_AGENT, |metadata| metadata.agent());
        format!("{} · {} · {agent}", row.tmux_name(), row.state().label())
    }

    fn worktree(row: &SessionSummary) -> String {
        row.metadata()
            .map(|metadata| metadata.worktree_path().display().to_string())
            .unwrap_or_default()
    }

    fn left_truncated(text: &str, width: usize) -> String {
        let length = text.chars().count();
        if length <= width {
            return text.to_string();
        }
        let kept: String = text.chars().skip(length + 1 - width).collect();
        format!("{}{kept}", Self::ELLIPSIS)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewCapture {
    Text(String),
    Unavailable(String),
}

pub struct PreviewText<'a> {
    raw: &'a str,
}

impl<'a> PreviewText<'a> {
    const ESCAPE: char = '\u{1b}';
    const BELL: char = '\u{7}';
    const CONTROL_SEQUENCE: char = '[';
    const OPERATING_SYSTEM_COMMAND: char = ']';
    const STRING_TERMINATOR: char = '\\';

    pub fn new(raw: &'a str) -> Self {
        Self { raw }
    }

    pub fn plain(&self) -> String {
        let mut plain = String::with_capacity(self.raw.len());
        let mut characters = self.raw.chars();
        while let Some(character) = characters.next() {
            if character != Self::ESCAPE {
                plain.push(character);
                continue;
            }
            match characters.next() {
                Some(Self::CONTROL_SEQUENCE) => {
                    Self::skip_through(&mut characters, |next| ('@'..='~').contains(&next))
                }
                Some(Self::OPERATING_SYSTEM_COMMAND) => {
                    Self::skip_through(&mut characters, |next| {
                        next == Self::BELL || next == Self::STRING_TERMINATOR
                    })
                }
                _ => {}
            }
        }
        plain
    }

    pub fn bottom_rows(&self, room: usize) -> Vec<String> {
        let plain = self.plain();
        let mut rows: Vec<&str> = plain.lines().collect();
        while rows.last().is_some_and(|row| row.trim().is_empty()) {
            rows.pop();
        }
        let first = rows.len().saturating_sub(room);
        rows[first..].iter().map(|row| row.to_string()).collect()
    }

    fn skip_through(characters: &mut impl Iterator<Item = char>, ends: impl Fn(char) -> bool) {
        for next in characters.by_ref() {
            if ends(next) {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::domain::SessionState;
    use crate::test_support::ServiceFixture;

    struct PanelFixture;

    impl PanelFixture {
        fn row() -> SessionSummary {
            SessionSummary::managed(
                ServiceFixture::session("fix-auth"),
                SessionState::Working,
                false,
            )
        }

        fn lines(capture: Option<&PreviewCapture>, width: u16, height: u16) -> Vec<String> {
            let row = Self::row();
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    PreviewPanel::new(Some(&row), capture, Theme).render(frame, frame.area())
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            (0..height)
                .map(|line| {
                    (2..width - 2)
                        .map(|column| buffer[(column, line)].symbol())
                        .collect::<String>()
                        .trim_end()
                        .to_string()
                })
                .collect()
        }
    }

    #[test]
    fn the_first_header_line_names_the_tmux_session_the_state_and_the_agent() {
        let lines = PanelFixture::lines(None, 50, 8);

        assert_eq!(lines[1], "ringleader-fix-auth · working · claude");
    }

    #[test]
    fn the_second_header_line_truncates_the_worktree_from_the_left() {
        let lines = PanelFixture::lines(None, 30, 8);

        assert_eq!(lines[2], "…/example-project/fix-auth");
    }

    #[test]
    fn captured_escape_sequences_are_removed() {
        let capture = PreviewCapture::Text(
            "\u{1b}[1;32m✻ Thinking…\u{1b}[0m\n\u{1b}]8;;https://example.com\u{7}link\u{1b}]8;;\u{1b}\\"
                .to_string(),
        );

        let lines = PanelFixture::lines(Some(&capture), 50, 8);

        assert_eq!(lines[4..6], ["✻ Thinking…", "link"]);
    }

    #[test]
    fn the_preview_keeps_the_last_rows_that_fit_without_trailing_blank_rows() {
        let capture = PreviewCapture::Text("one\ntwo\nthree\nfour\n\n  \n".to_string());

        let lines = PanelFixture::lines(Some(&capture), 50, 7);

        assert_eq!(lines[4..6], ["three", "four"]);
    }

    #[test]
    fn a_long_captured_row_is_clipped_without_wrapping() {
        let capture = PreviewCapture::Text(format!("{}\nnext", "x".repeat(80)));

        let lines = PanelFixture::lines(Some(&capture), 30, 8);

        assert_eq!(lines[4..6], ["x".repeat(26), "next".to_string()]);
    }

    #[test]
    fn a_failed_capture_shows_the_session_metadata() {
        let capture = PreviewCapture::Unavailable("can't find pane".to_string());

        let lines = PanelFixture::lines(Some(&capture), 50, 10);

        assert_eq!(lines[4], "No preview: can't find pane");
        assert_eq!(lines[5], "Repo: /repos/example-project");
        assert_eq!(lines[6], "Branch: ringleader/fix-auth");
    }
}

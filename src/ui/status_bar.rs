use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::text_fit::TextFit;
use super::theme::Theme;
use crate::domain::{SessionSummary, UnmanagedSession};

pub struct StatusBar<'a> {
    message: Option<&'a StatusMessage>,
    note: Option<String>,
    theme: Theme,
}

impl<'a> StatusBar<'a> {
    const MARGIN: &'static str = " ";
    const HEIGHT_SHARE: u16 = 4;

    pub fn new(theme: Theme) -> Self {
        Self {
            message: None,
            note: None,
            theme,
        }
    }

    pub fn with_message(self, message: Option<&'a StatusMessage>) -> Self {
        Self { message, ..self }
    }

    pub fn with_note(self, note: Option<String>) -> Self {
        Self { note, ..self }
    }

    pub fn height(&self, width: u16, screen_height: u16) -> u16 {
        let room = (screen_height / Self::HEIGHT_SHARE).max(1);
        self.rows(width).min(room)
    }

    pub fn rows(&self, width: u16) -> u16 {
        let count = self
            .text()
            .map_or(1, |(text, _)| Self::wrapped(text, width).len());
        u16::try_from(count).unwrap_or(u16::MAX)
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let lines: Vec<Line> = match self.text() {
            Some((text, style)) => Self::wrapped(text, area.width)
                .into_iter()
                .map(|line| Line::from(Span::styled(format!("{}{line}", Self::MARGIN), style)))
                .collect(),
            None => vec![Line::from(KeyHints::new(usize::from(area.width)).text())],
        };
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn text(&self) -> Option<(&str, Style)> {
        match (self.message, &self.note) {
            (Some(message), _) => Some((message.text(), message.style(&self.theme))),
            (None, Some(note)) => Some((note.as_str(), self.theme.warning())),
            (None, None) => None,
        }
    }

    fn wrapped(text: &str, width: u16) -> Vec<String> {
        let room = usize::from(width).saturating_sub(Self::MARGIN.len());
        TextFit::new(text).wrapped(room)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusMessage {
    Info(String),
    Error(String),
}

impl StatusMessage {
    pub fn text(&self) -> &str {
        match self {
            Self::Info(text) | Self::Error(text) => text,
        }
    }

    fn style(&self, theme: &Theme) -> Style {
        match self {
            Self::Info(_) => theme.base(),
            Self::Error(_) => theme.error(),
        }
    }
}

pub struct KeyHints {
    width: usize,
}

impl KeyHints {
    const HINTS: [(&'static str, &'static str); 9] = [
        ("n", "new"),
        ("d", "delete"),
        ("R", "rename"),
        ("Enter", "attach"),
        ("t", "shell"),
        ("s", "restart"),
        ("r", "refresh"),
        ("?", "help"),
        ("q", "quit"),
    ];
    const DROP_ORDER: [&'static str; 5] = ["r", "s", "t", "R", "d"];
    const WIDE_GAP: &'static str = "   ";
    const NARROW_GAP: &'static str = "  ";

    pub fn new(width: usize) -> Self {
        Self { width }
    }

    pub fn text(&self) -> String {
        let mut shown: Vec<String> = Self::HINTS
            .iter()
            .map(|(key, action)| format!("{key} {action}"))
            .collect();
        for dropped in Self::DROP_ORDER {
            if self.fits(&shown, Self::WIDE_GAP) {
                break;
            }
            shown.retain(|hint| hint.split(' ').next() != Some(dropped));
        }
        let gap = if self.fits(&shown, Self::WIDE_GAP) {
            Self::WIDE_GAP
        } else {
            Self::NARROW_GAP
        };
        Self::joined(&shown, gap)
    }

    fn fits(&self, hints: &[String], gap: &str) -> bool {
        Self::joined(hints, gap).chars().count() <= self.width
    }

    fn joined(hints: &[String], gap: &str) -> String {
        format!(" {}", hints.join(gap))
    }
}

pub struct StatusNote<'a> {
    row: &'a SessionSummary,
}

impl<'a> StatusNote<'a> {
    pub fn new(row: &'a SessionSummary) -> Self {
        Self { row }
    }

    pub fn text(&self) -> Option<String> {
        self.row.unmanaged_session().map(UnmanagedSession::note)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::domain::{PairMismatch, SessionObservation, SessionState, TmuxAddress};
    use crate::test_support::{FakeMultiplexer, ServiceFixture};

    struct BarFixture;

    impl BarFixture {
        fn render(status: StatusBar, width: u16) -> String {
            let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
            terminal
                .draw(|frame| status.render(frame, frame.area()))
                .unwrap();
            let buffer = terminal.backend().buffer();
            (0..width)
                .map(|column| buffer[(column, 0)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        }

        fn render_rows(status: StatusBar, width: u16, height: u16) -> Vec<String> {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| status.render(frame, frame.area()))
                .unwrap();
            let buffer = terminal.backend().buffer();
            (0..height)
                .map(|row| {
                    (0..width)
                        .map(|column| buffer[(column, row)].symbol())
                        .collect::<String>()
                })
                .collect()
        }

        fn note_for(row: &SessionSummary) -> String {
            let note = StatusNote::new(row).text();
            Self::render(StatusBar::new(Theme).with_note(note), 300)
        }
    }

    #[test]
    fn a_wide_bar_lists_every_key_in_the_fixed_order() {
        let text = BarFixture::render(StatusBar::new(Theme), 100);

        assert_eq!(
            text,
            " n new   d delete   R rename   Enter attach   t shell   s restart   r refresh   ? help   q quit"
        );
    }

    #[test]
    fn a_sixty_column_bar_drops_refresh_restart_shell_and_rename() {
        let text = BarFixture::render(StatusBar::new(Theme), 60);

        assert_eq!(text, " n new   d delete   Enter attach   ? help   q quit");
    }

    #[test]
    fn a_thirty_six_column_bar_keeps_the_fixed_keys_with_narrower_gaps() {
        let text = BarFixture::render(StatusBar::new(Theme), 36);

        assert_eq!(text, " n new  Enter attach  ? help  q quit");
    }

    #[test]
    fn a_long_message_wraps_onto_more_rows_and_a_short_one_takes_one() {
        let long = StatusMessage::Error("word ".repeat(40));
        let short = StatusMessage::Info("Reloaded the config.".to_string());

        let long_rows = StatusBar::new(Theme).with_message(Some(&long)).rows(80);
        let short_rows = StatusBar::new(Theme).with_message(Some(&short)).rows(80);
        let hint_rows = StatusBar::new(Theme).rows(80);

        assert_eq!((long_rows, short_rows, hint_rows), (3, 1, 1));
    }

    #[test]
    fn every_wrapped_row_keeps_the_left_margin() {
        let message = StatusMessage::Error("word ".repeat(40));
        let status = StatusBar::new(Theme).with_message(Some(&message));

        let rows = BarFixture::render_rows(status, 80, 3);

        assert!(rows.iter().all(|row| row.starts_with(" word")), "{rows:?}");
    }

    #[test]
    fn a_message_replaces_the_key_hints() {
        let message = StatusMessage::Error("config error".to_string());
        let status = StatusBar::new(Theme)
            .with_message(Some(&message))
            .with_note(Some("note".to_string()));

        assert_eq!(BarFixture::render(status, 60), " config error");
    }

    #[test]
    fn a_selected_shadowed_pair_shows_the_kill_session_advice() {
        let entry = ServiceFixture::session("fix-auth");
        let observation = ServiceFixture::waiting(&entry).without_metadata();
        let row = SessionSummary::unmanaged(UnmanagedSession::shadowing(
            entry,
            observation,
            PairMismatch::NoValidMetadata,
        ));

        let note = BarFixture::note_for(&row);

        assert!(note.contains(&format!(
            "end it with tmux -S {} kill-session -t '=ringleader-fix-auth:'",
            Path::new(FakeMultiplexer::SOCKET).display()
        )));
    }

    #[test]
    fn a_selected_session_without_metadata_is_attach_only() {
        let stranger = ServiceFixture::session("mystery");
        let row = SessionSummary::unmanaged(UnmanagedSession::new(
            "mystery",
            ServiceFixture::waiting(&stranger).without_metadata(),
        ));

        assert_eq!(
            BarFixture::note_for(&row),
            " ringleader-mystery has no valid Ringleader metadata, so it can only be attached."
        );
    }

    #[test]
    fn a_selected_session_without_the_prefix_says_its_name_lacks_it() {
        let session = ServiceFixture::session("fix-auth");
        let observation = SessionObservation::new(
            TmuxAddress::new("fixauth", Path::new(FakeMultiplexer::SOCKET)),
            ServiceFixture::CREATED_AT,
        )
        .with_metadata(session.metadata().clone());
        let row = SessionSummary::unmanaged(
            UnmanagedSession::unprefixed(observation).missing_only("ringleader-"),
        );

        assert_eq!(
            BarFixture::note_for(&row),
            " fixauth lacks the ringleader- prefix of Ringleader's sessions, so it can only be attached."
        );
    }

    #[test]
    fn a_selected_managed_session_has_no_note() {
        let row = SessionSummary::managed(
            ServiceFixture::session("fix-auth"),
            SessionState::Waiting,
            false,
        );

        assert_eq!(StatusNote::new(&row).text(), None);
    }
}

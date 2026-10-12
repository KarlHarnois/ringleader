use crossterm::event::{Event, KeyEventKind};
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::delete_session_dialog::DeleteSessionDialog;
use super::dialog_labels::DialogLabels;
use super::new_session_dialog::NewSessionDialog;
use super::rename_session_dialog::RenameSessionDialog;
use crate::adapters::app_log::AppLog;
use crate::application::{DeleteImpact, SessionService};
use crate::domain::{Session, SessionSummary};
use crate::ui::app::AppRequest;
use crate::ui::help_view::HelpView;
use crate::ui::home_view::LayoutTier;
use crate::ui::theme::Theme;

pub enum Dialog {
    NewSession(Box<NewSessionDialog>),
    Delete(Box<DeleteSessionDialog>),
    Rename(Box<RenameSessionDialog>),
    Help(Box<HelpView>),
}

impl Dialog {
    const WIDTH: u16 = 76;
    const HEIGHT: u16 = 22;

    pub fn open(opening: DialogOpening, service: &SessionService) -> Self {
        match opening {
            DialogOpening::NewSession => {
                Self::NewSession(Box::new(NewSessionDialog::open(service)))
            }
            DialogOpening::Rename(session) => {
                Self::Rename(Box::new(RenameSessionDialog::open(*session)))
            }
            DialogOpening::Help => Self::help(service),
        }
    }

    pub fn delete(
        row: Option<&SessionSummary>,
        name: &str,
        impact: DeleteImpact,
        changed: bool,
    ) -> Self {
        let dialog = DeleteSessionDialog::new(row, name, impact);
        Self::Delete(Box::new(if changed {
            dialog.after_a_change()
        } else {
            dialog
        }))
    }

    fn help(service: &SessionService) -> Self {
        let log_path = service.state_directory().join(AppLog::FILE_NAME);
        let tmux_prefix = service.config().naming().tmux_prefix();
        Self::Help(Box::new(HelpView::new(
            &service.config_path(),
            &log_path,
            tmux_prefix,
        )))
    }

    pub fn poll(&mut self) {
        if let Self::NewSession(dialog) = self {
            dialog.poll();
        }
    }

    pub fn shows_failures(&self) -> bool {
        matches!(self, Self::NewSession(_) | Self::Rename(_))
    }

    pub fn handle_event(&mut self, event: &Event, service: &SessionService) -> DialogStep {
        match (self, event) {
            (Self::NewSession(dialog), Event::Key(key)) if key.kind != KeyEventKind::Release => {
                dialog.handle_key(key, service)
            }
            (Self::NewSession(dialog), Event::Paste(text)) => {
                dialog.paste(text);
                DialogStep::Stay
            }
            (Self::Delete(dialog), Event::Key(key)) if key.kind != KeyEventKind::Release => {
                dialog.handle_key(key)
            }
            (Self::Rename(dialog), Event::Key(key)) if key.kind != KeyEventKind::Release => {
                dialog.handle_key(key)
            }
            (Self::Help(help), Event::Key(key)) if key.kind != KeyEventKind::Release => {
                help.handle_key(key)
            }
            (Self::Rename(dialog), Event::Paste(text)) => {
                dialog.paste(text);
                DialogStep::Stay
            }
            _ => DialogStep::Stay,
        }
    }

    pub fn fail(&mut self, error: String) {
        match self {
            Self::NewSession(dialog) => dialog.fail(error),
            Self::Rename(dialog) => dialog.fail(error),
            Self::Delete(_) | Self::Help(_) => {}
        }
    }

    pub fn render(&self, frame: &mut Frame, theme: &Theme) {
        let (area, labels) = Self::placement(frame.area());
        let block = Block::default().borders(Borders::ALL).title(self.title());
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        let (width, height) = (usize::from(inner.width), usize::from(inner.height));
        match self {
            Self::NewSession(dialog) => dialog.render(frame, inner, labels, theme),
            Self::Delete(dialog) => {
                Self::render_lines(frame, inner, dialog.lines(width, labels, theme))
            }
            Self::Rename(dialog) => {
                Self::render_lines(frame, inner, dialog.lines(width, labels, theme))
            }
            Self::Help(help) => Self::render_lines(frame, inner, help.lines(width, height, theme)),
        }
    }

    fn render_lines(frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>) {
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn title(&self) -> &'static str {
        match self {
            Self::NewSession(_) => NewSessionDialog::TITLE,
            Self::Delete(_) => DeleteSessionDialog::TITLE,
            Self::Rename(_) => RenameSessionDialog::TITLE,
            Self::Help(_) => HelpView::TITLE,
        }
    }

    fn placement(frame: Rect) -> (Rect, DialogLabels) {
        if LayoutTier::for_size(frame.width, frame.height) != LayoutTier::Full {
            let margin = Margin::new(1, 1);
            return (frame.inner(margin), DialogLabels::abbreviated());
        }
        let width = Self::WIDTH.min(frame.width);
        let height = Self::HEIGHT.min(frame.height.saturating_sub(2));
        let x = frame.x + (frame.width - width) / 2;
        let y = frame.y + (frame.height - height) / 2;
        (Rect::new(x, y, width, height), DialogLabels::full())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogOpening {
    NewSession,
    Rename(Box<Session>),
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogStep {
    Stay,
    Close,
    Request(AppRequest),
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::test_support::ServiceFixture;

    struct PlacementFixture;

    impl PlacementFixture {
        fn render(width: u16, height: u16) -> Terminal<TestBackend> {
            let service = ServiceFixture::new().service();
            let dialog = Dialog::open(DialogOpening::NewSession, &service);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| dialog.render(frame, &Theme)).unwrap();
            terminal
        }

        fn symbol(terminal: &Terminal<TestBackend>, column: u16, row: u16) -> String {
            terminal.backend().buffer()[(column, row)]
                .symbol()
                .to_string()
        }
    }

    #[test]
    fn the_full_tier_centers_the_dialog() {
        let terminal = PlacementFixture::render(100, 28);

        assert_eq!(PlacementFixture::symbol(&terminal, 12, 3), "┌");
        assert_eq!(PlacementFixture::symbol(&terminal, 13, 3), " ");
        assert_eq!(PlacementFixture::symbol(&terminal, 14, 3), "N");
        assert_eq!(PlacementFixture::symbol(&terminal, 13, 4), "N");
    }

    #[test]
    fn a_compact_tier_uses_the_frame_with_a_margin_and_abbreviated_labels() {
        let terminal = PlacementFixture::render(60, 12);

        assert_eq!(PlacementFixture::symbol(&terminal, 1, 1), "┌");
        assert_eq!(PlacementFixture::symbol(&terminal, 2, 2), "N");
        assert_eq!(PlacementFixture::symbol(&terminal, 3, 2), " ");
        assert_eq!(PlacementFixture::symbol(&terminal, 2, 3), "R");
    }
}

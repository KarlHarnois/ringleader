use std::time::Instant;

use crossterm::event::{Event, KeyEvent, KeyEventKind};
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph};

use super::delete_session_dialog::DeleteSessionDialog;
use super::dialog_labels::DialogLabels;
use super::home_path::HomePath;
use super::rename_session_dialog::RenameSessionDialog;
use super::session_finder::SessionFinder;
use super::usage_view::UsageView;
use crate::application::{DeleteImpact, SessionService};
use crate::domain::{Session, SessionSummary};
use crate::ui::app::AppRequest;
use crate::ui::dialogs::new_session::new_session_dialog::NewSessionDialog;
use crate::ui::help::help_view::HelpView;
use crate::ui::home::home_view::LayoutTier;
use crate::ui::theme::Theme;

pub enum Dialog {
    NewSession(Box<NewSessionDialog>),
    Delete(Box<DeleteSessionDialog>),
    Rename(Box<RenameSessionDialog>),
    Help(Box<HelpView>),
    FindSession(Box<SessionFinder>),
    Usage(Box<UsageView>),
}

impl Dialog {
    const WIDTH: u16 = 76;
    const HEIGHT: u16 = 22;
    const HELP_HEIGHT: u16 = 35;

    pub fn open(opening: DialogOpening, service: &SessionService) -> Self {
        match opening {
            DialogOpening::NewSession => {
                Self::NewSession(Box::new(NewSessionDialog::open(service)))
            }
            DialogOpening::Rename(session) => {
                Self::Rename(Box::new(RenameSessionDialog::open(*session)))
            }
            DialogOpening::Help => Self::help(service),
            DialogOpening::Usage => Self::Usage(Box::new(UsageView::open(service))),
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

    pub fn find_session(names: Vec<String>) -> Self {
        Self::FindSession(Box::new(SessionFinder::open(names)))
    }

    fn help(service: &SessionService) -> Self {
        let paths = service.paths();
        let config_path = service.config_path();
        let files = [
            ("Config", config_path.as_path()),
            ("Log", paths.log_file()),
            ("tmux socket", paths.tmux_socket()),
            ("tmux config", paths.tmux_config()),
        ];
        let home = HomePath::new(service.home_directory());
        Self::Help(Box::new(HelpView::new(
            &service.shown_key_bindings(),
            &files,
            &home,
        )))
    }

    pub fn poll(&mut self, now: Instant, service: &SessionService) {
        match self {
            Self::NewSession(dialog) => dialog.poll(),
            Self::Usage(view) => view.poll(now, service),
            Self::Delete(_) | Self::Rename(_) | Self::Help(_) | Self::FindSession(_) => {}
        }
    }

    pub fn shows_failures(&self) -> bool {
        matches!(self, Self::NewSession(_) | Self::Rename(_))
    }

    pub fn handle_event(&mut self, event: &Event, service: &SessionService) -> DialogStep {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.handle_key(key, service),
            Event::Paste(text) => {
                self.paste(text);
                DialogStep::Stay
            }
            _ => DialogStep::Stay,
        }
    }

    fn handle_key(&mut self, key: &KeyEvent, service: &SessionService) -> DialogStep {
        match self {
            Self::NewSession(dialog) => dialog.handle_key(key, service),
            Self::Delete(dialog) => dialog.handle_key(key),
            Self::Rename(dialog) => dialog.handle_key(key),
            Self::Help(help) => help.handle_key(key),
            Self::FindSession(finder) => finder.handle_key(key),
            Self::Usage(view) => view.handle_key(key),
        }
    }

    fn paste(&mut self, text: &str) {
        match self {
            Self::NewSession(dialog) => dialog.paste(text),
            Self::Rename(dialog) => dialog.paste(text),
            Self::FindSession(finder) => finder.paste(text),
            Self::Delete(_) | Self::Help(_) | Self::Usage(_) => {}
        }
    }

    pub fn fail(&mut self, error: String) {
        match self {
            Self::NewSession(dialog) => dialog.fail(error),
            Self::Rename(dialog) => dialog.fail(error),
            Self::Delete(_) | Self::Help(_) | Self::FindSession(_) | Self::Usage(_) => {}
        }
    }

    pub fn render(&self, frame: &mut Frame, theme: &Theme) {
        let (area, labels) = Self::placement(frame.area(), self.preferred_height());
        let block = Block::default()
            .borders(Borders::ALL)
            .title(self.title())
            .padding(self.padding());
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
            Self::FindSession(finder) => {
                Self::render_lines(frame, inner, finder.lines(width, height, theme))
            }
            Self::Usage(view) => Self::render_lines(frame, inner, view.lines(width, height, theme)),
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
            Self::FindSession(_) => SessionFinder::TITLE,
            Self::Usage(_) => UsageView::TITLE,
        }
    }

    fn padding(&self) -> Padding {
        match self {
            Self::NewSession(_) => Padding::horizontal(1),
            Self::Usage(_) => Padding::left(1),
            Self::Delete(_) | Self::Rename(_) | Self::Help(_) | Self::FindSession(_) => {
                Padding::ZERO
            }
        }
    }

    fn preferred_height(&self) -> u16 {
        match self {
            Self::Help(_) => Self::HELP_HEIGHT,
            Self::NewSession(_)
            | Self::Delete(_)
            | Self::Rename(_)
            | Self::FindSession(_)
            | Self::Usage(_) => Self::HEIGHT,
        }
    }

    fn placement(frame: Rect, preferred_height: u16) -> (Rect, DialogLabels) {
        if LayoutTier::for_size(frame.width, frame.height) != LayoutTier::Full {
            let margin = Margin::new(1, 1);
            return (frame.inner(margin), DialogLabels::abbreviated());
        }
        let width = Self::WIDTH.min(frame.width);
        let height = preferred_height.min(frame.height.saturating_sub(2));
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
    Usage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogStep {
    Stay,
    Close,
    Request(AppRequest),
    CloseWith(AppRequest),
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::test_support::ServiceFixture;

    struct PlacementFixture;

    impl PlacementFixture {
        fn render(opening: DialogOpening, width: u16, height: u16) -> Terminal<TestBackend> {
            let service = ServiceFixture::new().service();
            let dialog = Dialog::open(opening, &service);
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
        let terminal = PlacementFixture::render(DialogOpening::NewSession, 100, 28);

        assert_eq!(PlacementFixture::symbol(&terminal, 12, 3), "┌");
        assert_eq!(PlacementFixture::symbol(&terminal, 13, 3), " ");
        assert_eq!(PlacementFixture::symbol(&terminal, 14, 3), "N");
        assert_eq!(PlacementFixture::symbol(&terminal, 13, 4), " ");
        assert_eq!(PlacementFixture::symbol(&terminal, 14, 4), "N");
    }

    #[test]
    fn the_help_stands_taller_than_the_other_dialogs() {
        let help = PlacementFixture::render(DialogOpening::Help, 100, 32);
        let new_session = PlacementFixture::render(DialogOpening::NewSession, 100, 32);

        assert_eq!(PlacementFixture::symbol(&help, 12, 1), "┌");
        assert_eq!(PlacementFixture::symbol(&help, 12, 30), "└");
        assert_eq!(PlacementFixture::symbol(&new_session, 12, 5), "┌");
        assert_eq!(PlacementFixture::symbol(&new_session, 12, 26), "└");
    }

    #[test]
    fn the_usage_view_leaves_a_column_before_its_contents() {
        let terminal = PlacementFixture::render(DialogOpening::Usage, 100, 28);

        assert_eq!(PlacementFixture::symbol(&terminal, 12, 4), "│");
        assert_eq!(PlacementFixture::symbol(&terminal, 13, 4), " ");
        assert_eq!(PlacementFixture::symbol(&terminal, 14, 4), "M");
    }

    #[test]
    fn a_compact_tier_uses_the_frame_with_a_margin_and_abbreviated_labels() {
        let terminal = PlacementFixture::render(DialogOpening::NewSession, 60, 12);

        assert_eq!(PlacementFixture::symbol(&terminal, 1, 1), "┌");
        assert_eq!(PlacementFixture::symbol(&terminal, 2, 2), " ");
        assert_eq!(PlacementFixture::symbol(&terminal, 3, 2), "N");
        assert_eq!(PlacementFixture::symbol(&terminal, 4, 2), " ");
        assert_eq!(PlacementFixture::symbol(&terminal, 3, 3), "R");
    }
}

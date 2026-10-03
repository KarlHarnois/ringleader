use crossterm::event::{Event, KeyEventKind};
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::new_session_dialog::NewSessionDialog;
use crate::application::SessionService;
use crate::ui::app::AppRequest;
use crate::ui::home_view::LayoutTier;
use crate::ui::theme::Theme;

pub enum Dialog {
    NewSession(Box<NewSessionDialog>),
}

impl Dialog {
    const WIDTH: u16 = 76;
    const HEIGHT: u16 = 22;

    pub fn new_session(service: &SessionService) -> Self {
        Self::NewSession(Box::new(NewSessionDialog::open(service)))
    }

    pub fn poll(&mut self, service: &SessionService) {
        match self {
            Self::NewSession(dialog) => dialog.poll(service),
        }
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
            _ => DialogStep::Stay,
        }
    }

    pub fn fail(&mut self, error: String) {
        match self {
            Self::NewSession(dialog) => dialog.fail(error),
        }
    }

    pub fn render(&self, frame: &mut Frame, theme: &Theme) {
        let (area, labels) = Self::placement(frame.area());
        let block = Block::default().borders(Borders::ALL).title(self.title());
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        let (width, height) = (usize::from(inner.width), usize::from(inner.height));
        let lines = match self {
            Self::NewSession(dialog) => dialog.lines(width, height, labels, theme),
        };
        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn title(&self) -> &'static str {
        match self {
            Self::NewSession(_) => NewSessionDialog::TITLE,
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
pub enum DialogStep {
    Stay,
    Close,
    Request(AppRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogLabels {
    abbreviated: bool,
}

impl DialogLabels {
    const FULL_WIDTH: usize = 8;
    const ABBREVIATED_WIDTH: usize = 2;

    pub fn full() -> Self {
        Self { abbreviated: false }
    }

    pub fn abbreviated() -> Self {
        Self { abbreviated: true }
    }

    pub fn width(self) -> usize {
        if self.abbreviated {
            Self::ABBREVIATED_WIDTH
        } else {
            Self::FULL_WIDTH
        }
    }

    pub fn labeled(self, label: &str, lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
        let shown: String = if self.abbreviated {
            label.chars().take(1).collect()
        } else {
            label.to_string()
        };
        let width = self.width();
        lines
            .into_iter()
            .enumerate()
            .map(|(index, line)| {
                let prefix = if index == 0 { shown.as_str() } else { "" };
                let mut spans = vec![Span::raw(format!("{prefix:<width$}"))];
                spans.extend(
                    line.spans
                        .into_iter()
                        .map(|span| span.patch_style(line.style)),
                );
                Line::from(spans)
            })
            .collect()
    }
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
            let dialog = Dialog::new_session(&service);
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

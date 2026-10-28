use crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::Line;

use super::dialog::DialogStep;
use super::dialog_labels::DialogLabels;
use super::text_field::{FieldKind, TextField};
use crate::domain::Session;
use crate::ui::app::AppRequest;
use crate::ui::blocking_operation::BlockingOperation;
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct RenameSessionDialog {
    session: Session,
    new: TextField,
    error: Option<String>,
}

impl RenameSessionDialog {
    pub const TITLE: &'static str = " Rename session ";
    const HINTS: &'static str = "Enter rename · Esc cancel";
    const CURSOR: char = '█';
    const NEW_NAME_MISSING: &'static str = "enter a new name";

    pub fn open(session: Session) -> Self {
        Self {
            session,
            new: TextField::new(FieldKind::Name),
            error: None,
        }
    }

    pub fn fail(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> DialogStep {
        match key.code {
            KeyCode::Esc => DialogStep::Close,
            KeyCode::Enter => self.submit(),
            _ => {
                self.new.edit(key);
                DialogStep::Stay
            }
        }
    }

    pub fn paste(&mut self, text: &str) {
        self.error = self.new.paste(text).err();
    }

    pub fn lines(&self, width: usize, labels: DialogLabels, theme: &Theme) -> Vec<Line<'static>> {
        let room = width.saturating_sub(labels.width());
        let old = TextFit::new(self.session.name().as_str()).right_truncated(room);
        let new = self.new.visible(room, Some(Self::CURSOR));
        let mut lines = labels.labeled("Old", vec![Line::from(old)]);
        lines.extend(labels.labeled("New", vec![Line::from(new)]));
        lines.push(Line::from(""));
        let error = self.error.as_deref().unwrap_or_default();
        lines.extend(TextFit::new(error).styled_lines(width, theme.error()));
        lines.extend(TextFit::new(Self::HINTS).styled_lines(width, theme.hint()));
        lines
    }

    fn submit(&mut self) -> DialogStep {
        let new = self.new.value().trim();
        if new.is_empty() {
            self.error = Some(Self::NEW_NAME_MISSING.to_string());
            return DialogStep::Stay;
        }
        let operation = BlockingOperation::Rename {
            old: self.session.name().as_str().to_string(),
            new: new.to_string(),
        };
        DialogStep::Request(AppRequest::Block(operation))
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;

    use super::*;
    use crate::test_support::ServiceFixture;

    struct RenameFixture;

    impl RenameFixture {
        fn dialog() -> RenameSessionDialog {
            RenameSessionDialog::open(ServiceFixture::session("fix-auth"))
        }

        fn press(dialog: &mut RenameSessionDialog, code: KeyCode) -> DialogStep {
            dialog.handle_key(&KeyEvent::new(code, KeyModifiers::NONE))
        }

        fn type_text(dialog: &mut RenameSessionDialog, text: &str) {
            for character in text.chars() {
                Self::press(dialog, KeyCode::Char(character));
            }
        }

        fn text(dialog: &RenameSessionDialog) -> String {
            dialog
                .lines(70, DialogLabels::full(), &Theme::default())
                .iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        }

        fn rename(old: &str, new: &str) -> DialogStep {
            DialogStep::Request(AppRequest::Block(BlockingOperation::Rename {
                old: old.to_string(),
                new: new.to_string(),
            }))
        }
    }

    #[test]
    fn the_old_name_starts_with_the_selected_session() {
        let dialog = RenameFixture::dialog();

        assert!(RenameFixture::text(&dialog).starts_with("Old       fix-auth\nNew       █"));
    }

    #[test]
    fn the_dialog_shows_only_the_old_and_new_names_above_its_hints() {
        let mut dialog = RenameFixture::dialog();

        RenameFixture::type_text(&mut dialog, "fix-login");

        assert_eq!(
            RenameFixture::text(&dialog),
            "Old       fix-auth\nNew       fix-login█\n\n\nEnter rename · Esc cancel"
        );
    }

    #[test]
    fn submitting_renames_the_session() {
        let mut dialog = RenameFixture::dialog();
        RenameFixture::type_text(&mut dialog, "fix-login");

        let step = RenameFixture::press(&mut dialog, KeyCode::Enter);

        assert_eq!(step, RenameFixture::rename("fix-auth", "fix-login"));
    }

    #[test]
    fn a_pasted_name_turns_line_breaks_into_spaces() {
        let mut dialog = RenameFixture::dialog();

        dialog.paste("fix\nlogin");
        let step = RenameFixture::press(&mut dialog, KeyCode::Enter);

        assert_eq!(step, RenameFixture::rename("fix-auth", "fix login"));
    }

    #[test]
    fn an_empty_new_name_is_refused_in_the_dialog() {
        let mut dialog = RenameFixture::dialog();

        let step = RenameFixture::press(&mut dialog, KeyCode::Enter);

        assert_eq!(step, DialogStep::Stay);
        assert!(RenameFixture::text(&dialog).contains("enter a new name"));
    }

    #[test]
    fn esc_closes_the_dialog() {
        let mut dialog = RenameFixture::dialog();

        assert_eq!(
            RenameFixture::press(&mut dialog, KeyCode::Esc),
            DialogStep::Close
        );
    }
}

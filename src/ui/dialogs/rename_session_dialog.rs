use crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::Line;

use super::dialog::DialogStep;
use super::dialog_labels::DialogLabels;
use super::text_field::{FieldKind, TextField};
use crate::domain::{Session, SessionName};
use crate::ui::app::AppRequest;
use crate::ui::blocking_operation::BlockingOperation;
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct RenameSessionDialog {
    old: TextField,
    new: TextField,
    rename_branch: bool,
    focus: Field,
    sessions: Vec<Session>,
    error: Option<String>,
}

impl RenameSessionDialog {
    pub const TITLE: &'static str = " Rename session ";
    const HINTS: &'static str = "Tab next field · Space toggle · Enter rename · Esc cancel";
    const CURSOR: char = '█';

    pub fn open(selected: &str, sessions: Vec<Session>) -> Self {
        Self {
            old: TextField::new(FieldKind::Name).with_value(selected),
            new: TextField::new(FieldKind::Name),
            rename_branch: true,
            focus: Field::New,
            sessions,
            error: None,
        }
    }

    pub fn fail(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> DialogStep {
        match key.code {
            KeyCode::Esc => return DialogStep::Close,
            KeyCode::Enter => return self.submit(),
            KeyCode::Tab => self.focus = self.focus.step(1),
            KeyCode::BackTab => self.focus = self.focus.step(Field::ORDER.len() - 1),
            KeyCode::Char(' ') if self.focus == Field::Branch => {
                self.rename_branch = !self.rename_branch;
            }
            _ => {
                if let Some(field) = self.focused_field() {
                    field.edit(key);
                }
            }
        }
        DialogStep::Stay
    }

    pub fn paste(&mut self, text: &str) {
        if let Some(field) = self.focused_field() {
            self.error = field.paste(text).err();
        }
    }

    pub fn lines(&self, width: usize, labels: DialogLabels, theme: &Theme) -> Vec<Line<'static>> {
        let room = width.saturating_sub(labels.width());
        let toggle = if self.rename_branch {
            "[x] rename branch"
        } else {
            "[ ] rename branch"
        };
        let toggle_style = if self.focus == Field::Branch {
            theme.selected()
        } else {
            theme.base()
        };
        let mut lines = labels.labeled("Old", vec![self.text_line(Field::Old, &self.old, room)]);
        lines.extend(labels.labeled("New", vec![self.text_line(Field::New, &self.new, room)]));
        lines.extend(labels.labeled(
            "Branch",
            vec![
                Line::styled(toggle, toggle_style),
                Line::styled(self.branch_preview(), theme.dim()),
            ],
        ));
        lines.push(Line::from(""));
        let error = self.error.as_deref().unwrap_or_default();
        lines.extend(TextFit::new(error).styled_lines(width, theme.error()));
        lines.extend(TextFit::new(Self::HINTS).styled_lines(width, theme.dim()));
        lines
    }

    fn text_line(&self, field: Field, text: &TextField, room: usize) -> Line<'static> {
        let cursor = (self.focus == field).then_some(Self::CURSOR);
        Line::from(text.visible(room, cursor))
    }

    fn branch_preview(&self) -> String {
        let Some(session) = self.renamed_session() else {
            return format!("no session named {}", self.old.value());
        };
        let current = session.branch();
        if !self.rename_branch {
            return format!("{current} stays");
        }
        match SessionName::new(self.new.value()) {
            Ok(name) => format!("{current} → {}", session.naming().branch_name(&name)),
            Err(_) => format!("{current} → (enter a new name)"),
        }
    }

    fn renamed_session(&self) -> Option<&Session> {
        let old = self.old.value().trim();
        self.sessions
            .iter()
            .find(|session| session.name().as_str() == old)
    }

    fn focused_field(&mut self) -> Option<&mut TextField> {
        match self.focus {
            Field::Old => Some(&mut self.old),
            Field::New => Some(&mut self.new),
            Field::Branch => None,
        }
    }

    fn submit(&mut self) -> DialogStep {
        let old = self.old.value().trim().to_string();
        let new = self.new.value().trim().to_string();
        if old.is_empty() || new.is_empty() {
            self.error = Some("enter the old and the new name".to_string());
            return DialogStep::Stay;
        }
        let keep_branch = !self.rename_branch;
        let operation = BlockingOperation::Rename {
            old,
            new,
            keep_branch,
        };
        DialogStep::Request(AppRequest::Block(operation))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Old,
    New,
    Branch,
}

impl Field {
    const ORDER: [Field; 3] = [Field::Old, Field::New, Field::Branch];

    fn step(self, offset: usize) -> Self {
        let position = Self::ORDER.iter().position(|field| *field == self);
        Self::ORDER[(position.unwrap_or(0) + offset) % Self::ORDER.len()]
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
            let sessions = vec![
                ServiceFixture::session("fix-auth"),
                ServiceFixture::session("docs"),
            ];
            RenameSessionDialog::open("fix-auth", sessions)
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
                .lines(70, DialogLabels::full(), &Theme)
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

        fn rename(old: &str, new: &str, keep_branch: bool) -> DialogStep {
            DialogStep::Request(AppRequest::Block(BlockingOperation::Rename {
                old: old.to_string(),
                new: new.to_string(),
                keep_branch,
            }))
        }
    }

    #[test]
    fn the_old_name_starts_with_the_selected_session() {
        let dialog = RenameFixture::dialog();

        assert!(RenameFixture::text(&dialog).starts_with("Old       fix-auth\nNew       █"));
    }

    #[test]
    fn the_dialog_shows_the_branch_rename_that_will_happen() {
        let mut dialog = RenameFixture::dialog();

        RenameFixture::type_text(&mut dialog, "Fix Login");

        assert!(
            RenameFixture::text(&dialog).contains(
                "[x] rename branch\n          ringleader/fix-auth → ringleader/fix-login"
            )
        );
    }

    #[test]
    fn turning_the_toggle_off_keeps_the_branch() {
        let mut dialog = RenameFixture::dialog();
        RenameFixture::type_text(&mut dialog, "fix-login");
        RenameFixture::press(&mut dialog, KeyCode::Tab);

        RenameFixture::press(&mut dialog, KeyCode::Char(' '));
        let step = RenameFixture::press(&mut dialog, KeyCode::Enter);

        assert!(RenameFixture::text(&dialog).contains("[ ] rename branch"));
        assert!(RenameFixture::text(&dialog).contains("ringleader/fix-auth stays"));
        assert_eq!(step, RenameFixture::rename("fix-auth", "fix-login", true));
    }

    #[test]
    fn submitting_renames_the_branch_by_default() {
        let mut dialog = RenameFixture::dialog();
        RenameFixture::type_text(&mut dialog, "fix-login");

        let step = RenameFixture::press(&mut dialog, KeyCode::Enter);

        assert_eq!(step, RenameFixture::rename("fix-auth", "fix-login", false));
    }

    #[test]
    fn the_old_name_can_be_changed_to_another_session() {
        let mut dialog = RenameFixture::dialog();
        RenameFixture::press(&mut dialog, KeyCode::BackTab);
        for _ in "fix-auth".chars() {
            RenameFixture::press(&mut dialog, KeyCode::Backspace);
        }
        RenameFixture::type_text(&mut dialog, "docs");
        RenameFixture::press(&mut dialog, KeyCode::Tab);
        RenameFixture::type_text(&mut dialog, "guides");

        assert!(RenameFixture::text(&dialog).contains("ringleader/docs → ringleader/guides"));
    }

    #[test]
    fn a_pasted_name_turns_line_breaks_into_spaces() {
        let mut dialog = RenameFixture::dialog();

        dialog.paste("fix\nlogin");
        let step = RenameFixture::press(&mut dialog, KeyCode::Enter);

        assert_eq!(step, RenameFixture::rename("fix-auth", "fix login", false));
    }

    #[test]
    fn an_empty_new_name_is_refused_in_the_dialog() {
        let mut dialog = RenameFixture::dialog();

        let step = RenameFixture::press(&mut dialog, KeyCode::Enter);

        assert_eq!(step, DialogStep::Stay);
        assert!(RenameFixture::text(&dialog).contains("enter the old and the new name"));
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

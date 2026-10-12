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
    session: Session,
    new: TextField,
    rename_branch: bool,
    focus: Field,
    error: Option<String>,
}

impl RenameSessionDialog {
    pub const TITLE: &'static str = " Rename session ";
    const HINTS: &'static str = "Tab next field · Space toggle · Enter rename · Esc cancel";
    const CURSOR: char = '█';
    const NEW_NAME_MISSING: &'static str = "enter a new name";

    pub fn open(session: Session) -> Self {
        Self {
            session,
            new: TextField::new(FieldKind::Name),
            rename_branch: true,
            focus: Field::New,
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
            KeyCode::Tab | KeyCode::BackTab => self.focus = self.focus.other(),
            KeyCode::Char(' ') if self.focus == Field::Branch => {
                self.rename_branch = !self.rename_branch;
            }
            _ if self.focus == Field::New => {
                self.new.edit(key);
            }
            _ => {}
        }
        DialogStep::Stay
    }

    pub fn paste(&mut self, text: &str) {
        if self.focus == Field::New {
            self.error = self.new.paste(text).err();
        }
    }

    pub fn lines(&self, width: usize, labels: DialogLabels, theme: &Theme) -> Vec<Line<'static>> {
        let room = width.saturating_sub(labels.width());
        let old = TextFit::new(self.session.name().as_str()).right_truncated(room);
        let cursor = (self.focus == Field::New).then_some(Self::CURSOR);
        let mut lines = labels.labeled("Old", vec![Line::from(old)]);
        lines.extend(labels.labeled("New", vec![Line::from(self.new.visible(room, cursor))]));
        lines.extend(labels.labeled(
            "Branch",
            vec![
                self.toggle_line(theme),
                Line::styled(self.branch_preview(), theme.dim()),
            ],
        ));
        lines.push(Line::from(""));
        let error = self.error.as_deref().unwrap_or_default();
        lines.extend(TextFit::new(error).styled_lines(width, theme.error()));
        lines.extend(TextFit::new(Self::HINTS).styled_lines(width, theme.dim()));
        lines
    }

    fn toggle_line(&self, theme: &Theme) -> Line<'static> {
        let toggle = if self.rename_branch {
            "[x] rename branch"
        } else {
            "[ ] rename branch"
        };
        let style = if self.focus == Field::Branch {
            theme.selected()
        } else {
            theme.base()
        };
        Line::styled(toggle, style)
    }

    fn branch_preview(&self) -> String {
        let current = self.session.branch();
        if !self.rename_branch {
            return format!("{current} stays");
        }
        match SessionName::new(self.new.value()) {
            Ok(name) => format!("{current} → {}", self.session.naming().branch_name(&name)),
            Err(_) => format!("{current} → ({})", Self::NEW_NAME_MISSING),
        }
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
            keep_branch: !self.rename_branch,
        };
        DialogStep::Request(AppRequest::Block(operation))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    New,
    Branch,
}

impl Field {
    fn other(self) -> Self {
        match self {
            Field::New => Field::Branch,
            Field::Branch => Field::New,
        }
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
            RenameFixture::text(&dialog)
                .contains("[x] rename branch\n          fix-auth → fix-login")
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
        assert!(RenameFixture::text(&dialog).contains("fix-auth stays"));
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
    fn tab_moves_only_between_the_new_name_and_the_toggle() {
        let mut dialog = RenameFixture::dialog();

        RenameFixture::press(&mut dialog, KeyCode::Tab);
        RenameFixture::type_text(&mut dialog, "xyz");
        RenameFixture::press(&mut dialog, KeyCode::Tab);
        RenameFixture::type_text(&mut dialog, "docs");

        assert!(RenameFixture::text(&dialog).starts_with("Old       fix-auth\nNew       docs█"));
    }

    #[test]
    fn backtab_also_returns_to_the_new_name() {
        let mut dialog = RenameFixture::dialog();

        RenameFixture::press(&mut dialog, KeyCode::BackTab);
        RenameFixture::press(&mut dialog, KeyCode::BackTab);
        RenameFixture::type_text(&mut dialog, "docs");

        assert!(RenameFixture::text(&dialog).contains("New       docs█"));
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

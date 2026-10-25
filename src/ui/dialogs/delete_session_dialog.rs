use crossterm::event::KeyEvent;
use ratatui::text::Line;

use super::confirmation_keys::ConfirmationKeys;
use super::dialog::DialogStep;
use super::dialog_labels::DialogLabels;
use crate::application::DeleteImpact;
use crate::domain::SessionSummary;
use crate::ui::app::AppRequest;
use crate::ui::blocking_operation::{BlockingOperation, DeleteStep};
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct DeleteSessionDialog {
    name: String,
    details: Vec<(&'static str, String)>,
    impact: DeleteImpact,
    changed: bool,
    armed: bool,
}

impl DeleteSessionDialog {
    pub const TITLE: &'static str = " Delete session ";
    const CHANGED: &'static str =
        "What delete would lose changed since the dialog opened. Review it again.";
    const HINTS: &'static str = "Enter delete · Esc cancel";
    const ARMED_HINTS: &'static str = "This loses work. Press Enter again to delete · Esc cancel";
    const UNKNOWN: &'static str = "unknown";

    pub fn new(row: Option<&SessionSummary>, name: &str, impact: DeleteImpact) -> Self {
        let metadata = row.and_then(SessionSummary::metadata);
        let field = |value: Option<String>| value.unwrap_or_else(|| Self::UNKNOWN.to_string());
        let details = vec![
            ("Name", name.to_string()),
            (
                "Branch",
                field(metadata.map(|metadata| metadata.branch().to_string())),
            ),
            (
                "Worktree",
                field(metadata.map(|metadata| metadata.worktree_path().display().to_string())),
            ),
            (
                "Diff",
                field(
                    row.and_then(SessionSummary::diff)
                        .map(|diff| diff.to_string()),
                ),
            ),
        ];
        Self {
            name: name.to_string(),
            details,
            impact,
            changed: false,
            armed: false,
        }
    }

    pub fn after_a_change(self) -> Self {
        Self {
            changed: true,
            ..self
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> DialogStep {
        ConfirmationKeys::step(key, || self.confirm())
    }

    pub fn lines(&self, width: usize, labels: DialogLabels, theme: &Theme) -> Vec<Line<'static>> {
        let room = width.saturating_sub(labels.width());
        let mut lines: Vec<Line<'static>> = self
            .details
            .iter()
            .flat_map(|(label, value)| {
                labels.labeled(
                    label,
                    vec![Line::from(TextFit::new(value).left_truncated(room))],
                )
            })
            .collect();
        lines.push(Line::from(""));
        for warning in self.impact.warnings() {
            lines.extend(TextFit::new(&warning).styled_lines(width, theme.warning()));
        }
        if let Some(kept) = self.impact.kept() {
            lines.extend(TextFit::new(kept).styled_lines(width, theme.base()));
        }
        lines.push(Line::from(""));
        if self.changed {
            lines.extend(TextFit::new(Self::CHANGED).styled_lines(width, theme.error()));
        }
        let hints = if self.armed {
            Self::ARMED_HINTS
        } else {
            Self::HINTS
        };
        lines.extend(TextFit::new(hints).styled_lines(width, theme.dim()));
        lines
    }

    fn confirm(&mut self) -> DialogStep {
        if self.impact.needs_confirmation() && !self.armed {
            self.armed = true;
            return DialogStep::Stay;
        }
        DialogStep::Request(AppRequest::Block(BlockingOperation::Delete(
            DeleteStep::Confirmed {
                name: self.name.clone(),
                seen: self.impact.clone(),
            },
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crossterm::event::{KeyCode, KeyModifiers};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::application::ports::{DirtyState, ProcessInfo, UnmergedCommits};
    use crate::domain::{DiffStat, SessionState};
    use crate::test_support::ServiceFixture;
    use crate::ui::dialogs::Dialog;

    struct DeleteDialogFixture;

    impl DeleteDialogFixture {
        fn row() -> SessionSummary {
            SessionSummary::managed(
                ServiceFixture::session("fix-auth"),
                SessionState::Waiting,
                false,
            )
            .with_diff(DiffStat::from_changes([], 1))
        }

        fn risky_impact() -> DeleteImpact {
            DeleteImpact::measured(
                &DirtyState::present(
                    3,
                    vec![PathBuf::from(
                        "/data/ringleader/worktrees/example-project/fix-auth/vendor/lib",
                    )],
                ),
                &UnmergedCommits::new(2, Some(ServiceFixture::TIP), Some(ServiceFixture::TIP)),
                vec![ProcessInfo::new(4242, "vim notes.md")],
            )
            .while_working(true)
        }

        fn screen(dialog: Dialog) -> String {
            let mut terminal = Terminal::new(TestBackend::new(100, 28)).unwrap();
            terminal.draw(|frame| dialog.render(frame, &Theme)).unwrap();
            let buffer = terminal.backend().buffer();
            buffer
                .content()
                .chunks(100)
                .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        }

        fn press(dialog: &mut DeleteSessionDialog, code: KeyCode) -> DialogStep {
            dialog.handle_key(&KeyEvent::new(code, KeyModifiers::NONE))
        }

        fn delete_request(impact: DeleteImpact) -> DialogStep {
            DialogStep::Request(AppRequest::Block(BlockingOperation::Delete(
                DeleteStep::Confirmed {
                    name: "fix-auth".to_string(),
                    seen: impact,
                },
            )))
        }
    }

    #[test]
    fn the_dialog_shows_the_name_branch_worktree_and_diff() {
        let row = DeleteDialogFixture::row();

        let screen = DeleteDialogFixture::screen(Dialog::delete(
            Some(&row),
            "fix-auth",
            DeleteImpact::default(),
            false,
        ));

        assert!(screen.contains("Name      fix-auth"));
        assert!(screen.contains("Branch    fix-auth"));
        assert!(screen.contains("Worktree  /data/ringleader/worktrees/example-project/fix-auth"));
        assert!(screen.contains("Diff      +0 -0 ?1"));
    }

    #[test]
    fn the_dialog_names_the_nested_repository_and_the_outside_process() {
        let row = DeleteDialogFixture::row();
        let impact = DeleteDialogFixture::risky_impact();

        let screen =
            DeleteDialogFixture::screen(Dialog::delete(Some(&row), "fix-auth", impact, false));

        assert!(screen.contains("3 uncommitted files will be lost"));
        assert!(screen.contains("untracked nested repository"));
        assert!(screen.contains("fix-auth/vendor/lib"));
        assert!(screen.contains("2 commits exist on no other ref and will be lost"));
        assert!(screen.contains("process 4242 (vim notes.md) runs in the worktree"));
        assert!(screen.contains("agent is actively working"));
        assert!(screen.contains("ignored files such as .env are deleted with the worktree"));
    }

    #[test]
    fn the_dialog_of_a_session_in_the_main_checkout_says_the_checkout_stays() {
        let row = SessionSummary::managed(
            ServiceFixture::main_checkout_session("fix-auth"),
            SessionState::Waiting,
            false,
        );

        let screen = DeleteDialogFixture::screen(Dialog::delete(
            Some(&row),
            "fix-auth",
            DeleteImpact::session_only(),
            false,
        ));

        assert!(
            screen.contains("the main checkout and its branch stay, delete ends only the session")
        );
        assert!(!screen.contains(".env"));
    }

    #[test]
    fn a_clean_session_is_deleted_on_the_first_confirmation() {
        let mut dialog = DeleteSessionDialog::new(None, "fix-auth", DeleteImpact::default());

        let step = DeleteDialogFixture::press(&mut dialog, KeyCode::Enter);

        assert_eq!(
            step,
            DeleteDialogFixture::delete_request(DeleteImpact::default())
        );
    }

    #[test]
    fn work_at_risk_needs_a_second_confirmation() {
        let impact = DeleteDialogFixture::risky_impact();
        let mut dialog = DeleteSessionDialog::new(None, "fix-auth", impact.clone());

        let first = DeleteDialogFixture::press(&mut dialog, KeyCode::Enter);
        let second = DeleteDialogFixture::press(&mut dialog, KeyCode::Enter);

        assert_eq!(first, DialogStep::Stay);
        assert_eq!(second, DeleteDialogFixture::delete_request(impact));
    }

    #[test]
    fn an_armed_dialog_says_the_next_enter_loses_work() {
        let mut dialog =
            DeleteSessionDialog::new(None, "fix-auth", DeleteDialogFixture::risky_impact());

        DeleteDialogFixture::press(&mut dialog, KeyCode::Enter);
        let screen = DeleteDialogFixture::screen(Dialog::Delete(Box::new(dialog)));

        assert!(screen.contains("This loses work. Press Enter again to delete"));
    }

    #[test]
    fn esc_closes_the_dialog_without_deleting() {
        let mut dialog = DeleteSessionDialog::new(None, "fix-auth", DeleteImpact::default());

        assert_eq!(
            DeleteDialogFixture::press(&mut dialog, KeyCode::Esc),
            DialogStep::Close
        );
    }

    #[test]
    fn a_reopened_dialog_says_what_delete_would_lose_changed() {
        let screen = DeleteDialogFixture::screen(Dialog::delete(
            None,
            "fix-auth",
            DeleteImpact::default(),
            true,
        ));

        assert!(screen.contains("What delete would lose changed since the dialog opened."));
    }
}

use crossterm::event::KeyEvent;
use ratatui::text::Line;

use super::confirmation_keys::ConfirmationKeys;
use super::dialog::DialogStep;
use crate::application::{BatchReport, DeleteImpact, ServiceError};
use crate::ui::app::AppRequest;
use crate::ui::batch_operation::{BatchOperation, DeleteSessionsStep};
use crate::ui::blocking_operation::BlockingOperation;
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct DeleteSessionsDialog {
    measured: BatchReport<DeleteImpact>,
    armed: bool,
}

impl DeleteSessionsDialog {
    pub const TITLE: &'static str = " Delete sessions ";
    const HINTS: &'static str = "Enter delete · Esc cancel";
    const ARMED_HINTS: &'static str = "This loses work. Press Enter again to delete · Esc cancel";
    const NOTHING_TO_DELETE: &'static str = "No selected session can be deleted · Esc cancel";

    pub fn new(measured: BatchReport<DeleteImpact>) -> Self {
        Self {
            measured,
            armed: false,
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> DialogStep {
        ConfirmationKeys::step(key, || self.confirm())
    }

    pub fn lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = self
            .measured
            .outcomes()
            .flat_map(|(name, impact)| Self::session_lines(name, impact, width, theme))
            .collect();
        lines.push(Line::from(""));
        lines.extend(TextFit::new(self.hints()).styled_lines(width, theme.dim()));
        lines
    }

    fn session_lines(
        name: &str,
        impact: Result<&DeleteImpact, &ServiceError>,
        width: usize,
        theme: &Theme,
    ) -> Vec<Line<'static>> {
        match impact {
            Ok(impact) => Self::measured_lines(name, impact, width, theme),
            Err(error) => {
                let skipped = format!("{name} is skipped: {error}");
                TextFit::new(&skipped).styled_lines(width, theme.error())
            }
        }
    }

    fn measured_lines(
        name: &str,
        impact: &DeleteImpact,
        width: usize,
        theme: &Theme,
    ) -> Vec<Line<'static>> {
        let heading = TextFit::new(name).styled_lines(width, theme.emphasis());
        let warnings = impact
            .warnings()
            .into_iter()
            .flat_map(|warning| TextFit::new(&warning).styled_lines(width, theme.warning()));
        heading.into_iter().chain(warnings).collect()
    }

    fn hints(&self) -> &'static str {
        if self.deletable().is_empty() {
            return Self::NOTHING_TO_DELETE;
        }
        match self.armed {
            true => Self::ARMED_HINTS,
            false => Self::HINTS,
        }
    }

    fn confirm(&mut self) -> DialogStep {
        let deletable = self.deletable();
        if deletable.is_empty() {
            return DialogStep::Stay;
        }
        if self.needs_confirmation() && !self.armed {
            self.armed = true;
            return DialogStep::Stay;
        }
        DialogStep::Request(AppRequest::Block(BlockingOperation::Batch(
            BatchOperation::Delete(DeleteSessionsStep::Confirmed(deletable)),
        )))
    }

    fn needs_confirmation(&self) -> bool {
        self.measured
            .succeeded()
            .any(|(_, impact)| impact.needs_confirmation())
    }

    fn deletable(&self) -> Vec<(String, DeleteImpact)> {
        self.measured
            .succeeded()
            .map(|(name, impact)| (name.to_string(), impact.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use super::*;
    use crate::application::ports::{DirtyState, UnmergedCommits};
    use crate::test_support::ServiceFixture;

    struct BatchDeleteFixture;

    impl BatchDeleteFixture {
        fn clean(name: &str) -> (String, Result<DeleteImpact, ServiceError>) {
            (name.to_string(), Ok(DeleteImpact::default()))
        }

        fn dirty(name: &str) -> (String, Result<DeleteImpact, ServiceError>) {
            let impact = DeleteImpact::measured(
                &DirtyState::present(3, Vec::new()),
                &UnmergedCommits::new(2, Some(ServiceFixture::TIP), Some(ServiceFixture::TIP)),
                Vec::new(),
            );
            (name.to_string(), Ok(impact))
        }

        fn unmeasurable(name: &str) -> (String, Result<DeleteImpact, ServiceError>) {
            let error = ServiceError::Unmeasurable(format!("the worktree of {name} is gone"));
            (name.to_string(), Err(error))
        }

        fn dialog(
            measured: Vec<(String, Result<DeleteImpact, ServiceError>)>,
        ) -> DeleteSessionsDialog {
            DeleteSessionsDialog::new(BatchReport::new(measured))
        }

        fn enter(dialog: &mut DeleteSessionsDialog) -> DialogStep {
            dialog.handle_key(&KeyEvent::from(KeyCode::Enter))
        }

        fn text(dialog: &DeleteSessionsDialog) -> String {
            let lines = dialog.lines(80, &Theme);
            let rows: Vec<String> = lines.iter().map(ToString::to_string).collect();
            rows.join("\n")
        }

        fn deletes(seen: Vec<(String, DeleteImpact)>) -> DialogStep {
            DialogStep::Request(AppRequest::Block(BlockingOperation::Batch(
                BatchOperation::Delete(DeleteSessionsStep::Confirmed(seen)),
            )))
        }
    }

    #[test]
    fn enter_deletes_every_clean_session_at_once() {
        let mut dialog = BatchDeleteFixture::dialog(vec![
            BatchDeleteFixture::clean("alpha"),
            BatchDeleteFixture::clean("bravo"),
        ]);

        assert_eq!(
            BatchDeleteFixture::enter(&mut dialog),
            BatchDeleteFixture::deletes(vec![
                ("alpha".to_string(), DeleteImpact::default()),
                ("bravo".to_string(), DeleteImpact::default()),
            ])
        );
    }

    #[test]
    fn a_session_that_would_lose_work_arms_a_second_enter() {
        let mut dialog = BatchDeleteFixture::dialog(vec![
            BatchDeleteFixture::clean("alpha"),
            BatchDeleteFixture::dirty("bravo"),
        ]);

        assert_eq!(BatchDeleteFixture::enter(&mut dialog), DialogStep::Stay);
        assert!(BatchDeleteFixture::text(&dialog).contains("Press Enter again"));
        assert!(matches!(
            BatchDeleteFixture::enter(&mut dialog),
            DialogStep::Request(_)
        ));
    }

    #[test]
    fn the_dialog_names_each_session_with_what_it_would_lose() {
        let dialog = BatchDeleteFixture::dialog(vec![BatchDeleteFixture::dirty("bravo")]);

        let text = BatchDeleteFixture::text(&dialog);

        assert!(text.contains("bravo"));
        assert!(text.contains("3 uncommitted files will be lost"), "{text}");
    }

    #[test]
    fn an_unmeasurable_session_is_listed_as_skipped_and_left_out_of_the_delete() {
        let mut dialog = BatchDeleteFixture::dialog(vec![
            BatchDeleteFixture::unmeasurable("alpha"),
            BatchDeleteFixture::clean("bravo"),
        ]);

        assert!(
            BatchDeleteFixture::text(&dialog)
                .contains("alpha is skipped: the worktree of alpha is gone")
        );
        assert_eq!(
            BatchDeleteFixture::enter(&mut dialog),
            BatchDeleteFixture::deletes(vec![("bravo".to_string(), DeleteImpact::default())])
        );
    }

    #[test]
    fn a_dialog_with_nothing_deletable_stays_open_on_enter() {
        let mut dialog =
            BatchDeleteFixture::dialog(vec![BatchDeleteFixture::unmeasurable("alpha")]);

        assert_eq!(BatchDeleteFixture::enter(&mut dialog), DialogStep::Stay);
        assert!(BatchDeleteFixture::text(&dialog).contains("No selected session can be deleted"));
    }

    #[test]
    fn esc_closes_the_dialog() {
        let mut dialog = BatchDeleteFixture::dialog(vec![BatchDeleteFixture::clean("alpha")]);

        assert_eq!(
            dialog.handle_key(&KeyEvent::from(KeyCode::Esc)),
            DialogStep::Close
        );
    }
}

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::Line;

use super::add_repo_dialog::AddRepoDialog;
use super::picker_row::PickerRow;
use super::repo_scan::RepoScan;
use super::scope_input::ScopeInput;
use crate::application::SessionService;
use crate::domain::Repo;
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct RepoPicker {
    scan: RepoScan,
    cursor: usize,
    chosen: Option<usize>,
    form: Option<PickerForm>,
    select_after_scan: Option<PathBuf>,
}

impl RepoPicker {
    const NO_REPO: &'static str = "no repo selected";
    const CHOSEN: &'static str = "> ";
    const UNCHOSEN: &'static str = "  ";
    const FORM_HEIGHT: usize = 3;

    pub fn open(service: &SessionService) -> Self {
        Self {
            scan: RepoScan::start(service, service.launch_directory()),
            cursor: 0,
            chosen: None,
            form: None,
            select_after_scan: None,
        }
    }

    pub fn has_open_form(&self) -> bool {
        self.form.is_some()
    }

    pub fn selected_repo(&self) -> Option<&Repo> {
        self.scan.repos().get(self.chosen?)
    }

    pub fn summary(&self) -> String {
        match self.selected_repo() {
            _ if self.scan.is_scanning() => self.scan.header(),
            Some(repo) => repo.name().to_string(),
            None => Self::NO_REPO.to_string(),
        }
    }

    pub fn poll(&mut self, service: &mut SessionService) {
        match &mut self.form {
            Some(PickerForm::Scope(input)) => {
                if let Some(scope) = input.checked_path() {
                    self.form = None;
                    self.scan = RepoScan::start(service, &scope);
                }
            }
            Some(PickerForm::AddRepo(dialog)) => {
                if let Some(repo) = dialog.poll(service) {
                    match service.reload() {
                        Ok(()) => self.rescan_selecting(service, &repo),
                        Err(error) => dialog.fail(error.to_string()),
                    }
                }
            }
            None => {}
        }
        if self.scan.finish_if_done() {
            self.select_scanned();
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent, service: &SessionService) -> bool {
        if let Some(form) = &mut self.form {
            if key.code == KeyCode::Esc {
                self.form = None;
                return true;
            }
            return form.handle_key(key, service);
        }
        let rows = self.rows().len();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_cursor_to(self.cursor.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_cursor_to((self.cursor + 1).min(rows.saturating_sub(1)));
            }
            KeyCode::Enter if self.cursor + 2 == rows => {
                self.form = Some(PickerForm::AddRepo(AddRepoDialog::open()));
            }
            KeyCode::Enter if self.cursor + 1 == rows => {
                self.form = Some(PickerForm::Scope(ScopeInput::default()));
            }
            _ => return false,
        }
        true
    }

    pub fn paste(&mut self, text: &str) {
        if let Some(form) = &mut self.form {
            form.paste(text);
        }
    }

    pub fn lines(&self, width: usize, room: usize, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = vec![Line::from(self.scan.header())];
        let notices = self.scan.notices().into_iter();
        lines.extend(
            notices.flat_map(|notice| TextFit::new(&notice).styled_lines(width, theme.dim())),
        );
        if let Some(problem) = self.scan.problem() {
            lines.extend(TextFit::new(&problem).styled_lines(width, theme.error()));
        }
        let rows = self.rows();
        let room = room.saturating_sub(lines.len() + Self::FORM_HEIGHT).max(1);
        let first = (self.cursor + 1).saturating_sub(room);
        for (index, row) in rows.iter().enumerate().skip(first).take(room) {
            let mark = if Some(index) == self.chosen {
                Self::CHOSEN
            } else {
                Self::UNCHOSEN
            };
            let text = format!(
                "{mark}{}",
                row.text(&rows, width.saturating_sub(mark.len()))
            );
            let style = if index == self.cursor {
                theme.selected()
            } else {
                theme.base()
            };
            lines.push(Line::styled(text, style));
        }
        if let Some(form) = &self.form {
            lines.extend(form.lines(width, theme));
        }
        lines
    }

    fn rows(&self) -> Vec<PickerRow<'_>> {
        let repos = self.scan.repos().iter().map(PickerRow::Repo);
        repos
            .chain([PickerRow::AddRepo, PickerRow::ChangeScope])
            .collect()
    }

    fn move_cursor_to(&mut self, row: usize) {
        self.cursor = row;
        if row < self.scan.repos().len() {
            self.chosen = Some(row);
        } else if self.scan.repos().is_empty() {
            self.chosen = None;
        }
    }

    fn rescan_selecting(&mut self, service: &SessionService, repo: &Repo) {
        self.form = None;
        self.select_after_scan = Some(repo.path().to_path_buf());
        self.scan = self.scan.again(service);
    }

    fn select_scanned(&mut self) {
        let wanted = self.select_after_scan.take();
        let repos = self.scan.repos();
        let position = repos
            .iter()
            .position(|repo| Some(repo.path()) == wanted.as_deref());
        self.move_cursor_to(position.unwrap_or(0));
    }
}

enum PickerForm {
    Scope(ScopeInput),
    AddRepo(AddRepoDialog),
}

impl PickerForm {
    fn handle_key(&mut self, key: &KeyEvent, service: &SessionService) -> bool {
        match self {
            Self::Scope(input) => input.handle_key(key, service),
            Self::AddRepo(dialog) => dialog.handle_key(key, service),
        }
    }

    fn paste(&mut self, text: &str) {
        match self {
            Self::Scope(input) => input.paste(text),
            Self::AddRepo(dialog) => dialog.paste(text),
        }
    }

    fn lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        match self {
            Self::Scope(input) => input.lines(width, theme),
            Self::AddRepo(dialog) => dialog.lines(width, theme),
        }
    }
}

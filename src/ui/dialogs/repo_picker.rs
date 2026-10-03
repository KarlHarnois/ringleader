use crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::Line;

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
    scope_input: Option<ScopeInput>,
}

impl RepoPicker {
    const CHANGE_SCOPE: &'static str = "Change scope…";
    const CONFIG_MARK: &'static str = " · config";
    const NO_REPO: &'static str = "no repo selected";
    const CHOSEN: &'static str = "> ";
    const UNCHOSEN: &'static str = "  ";
    const PATH_LABEL: usize = "Path: ".len();

    pub fn open(service: &SessionService) -> Self {
        Self {
            scan: RepoScan::start(service, service.launch_directory()),
            cursor: 0,
            chosen: None,
            scope_input: None,
        }
    }

    pub fn is_editing_scope(&self) -> bool {
        self.scope_input.is_some()
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

    pub fn poll(&mut self, service: &SessionService) {
        let checked_scope = self.scope_input.as_mut().and_then(ScopeInput::checked_path);
        if let Some(scope) = checked_scope {
            self.scope_input = None;
            self.scan = RepoScan::start(service, &scope);
            self.move_cursor_to(0);
        }
        if self.scan.finish_if_done() {
            self.move_cursor_to(0);
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent, service: &SessionService) -> bool {
        if let Some(input) = &mut self.scope_input {
            if key.code == KeyCode::Esc {
                self.scope_input = None;
                return true;
            }
            return input.handle_key(key, service);
        }
        let last = self.rows().len().saturating_sub(1);
        match key.code {
            KeyCode::Up => self.move_cursor_to(self.cursor.saturating_sub(1)),
            KeyCode::Down => self.move_cursor_to((self.cursor + 1).min(last)),
            KeyCode::Enter if self.cursor == last => {
                self.scope_input = Some(ScopeInput::default());
            }
            _ => return false,
        }
        true
    }

    fn move_cursor_to(&mut self, row: usize) {
        self.cursor = row;
        if row < self.scan.repos().len() {
            self.chosen = Some(row);
        } else if self.scan.repos().is_empty() {
            self.chosen = None;
        }
    }

    pub fn paste(&mut self, text: &str) {
        if let Some(input) = &mut self.scope_input {
            input.paste(text);
        }
    }

    pub fn lines(&self, width: usize, room: usize, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = vec![Line::from(self.scan.header())];
        let notices = self.scan.notices().into_iter();
        lines.extend(notices.map(|notice| Line::styled(notice, theme.dim())));
        if let Some(problem) = self.scan.problem() {
            lines.push(Line::styled(problem, theme.error()));
        }
        let rows = self.rows();
        let room = room.saturating_sub(lines.len() + ScopeInput::HEIGHT).max(1);
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
        if let Some(input) = &self.scope_input {
            lines.extend(input.lines(width.saturating_sub(Self::PATH_LABEL), theme));
        }
        lines
    }

    fn rows(&self) -> Vec<PickerRow<'_>> {
        let repos = self.scan.repos().iter().map(PickerRow::Repo);
        repos.chain([PickerRow::ChangeScope]).collect()
    }
}

enum PickerRow<'a> {
    Repo(&'a Repo),
    ChangeScope,
}

impl PickerRow<'_> {
    fn text(&self, rows: &[PickerRow], width: usize) -> String {
        let PickerRow::Repo(repo) = self else {
            return RepoPicker::CHANGE_SCOPE.to_string();
        };
        let mut text = repo.name().to_string();
        if repo.is_explicit() {
            text.push_str(RepoPicker::CONFIG_MARK);
        }
        let namesakes = rows
            .iter()
            .filter(|other| matches!(other, PickerRow::Repo(other) if other.name() == repo.name()));
        if namesakes.count() > 1 {
            let room = width.saturating_sub(text.chars().count() + 2);
            let path = repo.path().display().to_string();
            text.push_str(&format!("  {}", TextFit::new(&path).left_truncated(room)));
        }
        text
    }
}

use crate::domain::Repo;
use crate::ui::text_fit::TextFit;

pub enum PickerRow<'a> {
    Repo(&'a Repo),
    AddRepo,
    ChangeScope,
}

impl PickerRow<'_> {
    const ADD_REPO: &'static str = "Add repo…";
    const CHANGE_SCOPE: &'static str = "Change scope…";
    const CONFIG_MARK: &'static str = " · config";

    pub fn text(&self, rows: &[PickerRow], width: usize) -> String {
        let repo = match self {
            PickerRow::Repo(repo) => repo,
            PickerRow::AddRepo => return Self::ADD_REPO.to_string(),
            PickerRow::ChangeScope => return Self::CHANGE_SCOPE.to_string(),
        };
        let mut text = repo.name().to_string();
        if repo.is_explicit() {
            text.push_str(Self::CONFIG_MARK);
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

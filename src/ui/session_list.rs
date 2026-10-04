use super::session_table::SessionTable;
use super::theme::Theme;
use crate::domain::{Session, SessionSummary};

#[derive(Debug, Default)]
pub struct SessionList {
    rows: Vec<SessionSummary>,
    selection: Option<String>,
}

impl SessionList {
    pub fn rows(&self) -> &[SessionSummary] {
        &self.rows
    }

    pub fn table(&self, tick: u64, theme: Theme) -> SessionTable<'_> {
        SessionTable::new(&self.rows, self.selected_name(), tick, theme)
    }

    pub fn selected_name(&self) -> Option<&str> {
        self.selected().map(SessionSummary::name)
    }

    pub fn managed_sessions(&self) -> Vec<Session> {
        self.rows
            .iter()
            .filter_map(SessionSummary::session)
            .cloned()
            .collect()
    }

    pub fn row(&self, name: &str) -> Option<&SessionSummary> {
        self.rows.iter().find(|row| row.name() == name)
    }

    pub fn selected(&self) -> Option<&SessionSummary> {
        let selected = self.selection.as_deref()?;
        self.rows.iter().find(|row| row.name() == selected)
    }

    pub fn show(&mut self, mut rows: Vec<SessionSummary>) {
        rows.sort_by(|first, second| {
            (first.state(), first.name()).cmp(&(second.state(), second.name()))
        });
        self.rows = rows;
        if self.selected().is_none() {
            self.selection = self.rows.first().map(|row| row.name().to_string());
        }
    }

    pub fn select(&mut self, name: &str) {
        if self.rows.iter().any(|row| row.name() == name) {
            self.selection = Some(name.to_string());
        }
    }

    pub fn move_selection(&mut self, offset: isize) {
        let current = self.selected_index().unwrap_or(0);
        let last = self.rows.len().saturating_sub(1);
        let target = current.saturating_add_signed(offset).min(last);
        if let Some(row) = self.rows.get(target) {
            self.selection = Some(row.name().to_string());
        }
    }

    fn selected_index(&self) -> Option<usize> {
        let selected = self.selection.as_deref()?;
        self.rows.iter().position(|row| row.name() == selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::SessionState;
    use crate::test_support::ServiceFixture;

    struct ListFixture;

    impl ListFixture {
        fn row(name: &str, state: SessionState) -> SessionSummary {
            SessionSummary::managed(ServiceFixture::session(name), state, false)
        }

        fn names(list: &SessionList) -> Vec<&str> {
            list.rows().iter().map(SessionSummary::name).collect()
        }
    }

    #[test]
    fn rows_sort_by_state_priority_then_name() {
        let mut list = SessionList::default();

        list.show(vec![
            ListFixture::row("mystery", SessionState::Unmanaged),
            ListFixture::row("flaky-test", SessionState::Stopped),
            ListFixture::row("docs", SessionState::Exited),
            ListFixture::row("dark-mode", SessionState::Waiting),
            ListFixture::row("spike-tui", SessionState::Working),
            ListFixture::row("fix-auth", SessionState::Working),
        ]);

        assert_eq!(
            ListFixture::names(&list),
            [
                "fix-auth",
                "spike-tui",
                "dark-mode",
                "docs",
                "flaky-test",
                "mystery"
            ]
        );
    }

    #[test]
    fn the_first_row_is_selected_when_nothing_was() {
        let mut list = SessionList::default();

        list.show(vec![ListFixture::row("fix-auth", SessionState::Waiting)]);

        assert_eq!(list.selected_name(), Some("fix-auth"));
    }

    #[test]
    fn the_selection_keeps_its_name_when_its_row_moves() {
        let mut list = SessionList::default();
        list.show(vec![
            ListFixture::row("dark-mode", SessionState::Waiting),
            ListFixture::row("fix-auth", SessionState::Waiting),
        ]);
        list.move_selection(1);

        list.show(vec![
            ListFixture::row("dark-mode", SessionState::Exited),
            ListFixture::row("fix-auth", SessionState::Waiting),
        ]);

        assert_eq!(list.selected_name(), Some("fix-auth"));
        assert_eq!(ListFixture::names(&list), ["fix-auth", "dark-mode"]);
    }

    #[test]
    fn a_vanished_selection_falls_back_to_the_first_row() {
        let mut list = SessionList::default();
        list.show(vec![
            ListFixture::row("dark-mode", SessionState::Waiting),
            ListFixture::row("fix-auth", SessionState::Waiting),
        ]);
        list.move_selection(1);

        list.show(vec![ListFixture::row("dark-mode", SessionState::Waiting)]);

        assert_eq!(list.selected_name(), Some("dark-mode"));
    }

    #[test]
    fn moving_stops_at_either_end() {
        let mut list = SessionList::default();
        list.show(vec![
            ListFixture::row("dark-mode", SessionState::Waiting),
            ListFixture::row("fix-auth", SessionState::Waiting),
        ]);

        list.move_selection(-1);
        let at_top = list.selected_name().map(str::to_string);
        list.move_selection(5);

        assert_eq!(at_top.as_deref(), Some("dark-mode"));
        assert_eq!(list.selected_name(), Some("fix-auth"));
    }
}

use std::collections::HashMap;

use super::session_table::SessionTable;
use super::theme::Theme;
use crate::domain::{DiffStat, PullRequest, SessionSummary};

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

    pub fn visible_managed(&self, room: usize) -> Vec<String> {
        let first = SessionTable::first_visible(self.selected_index().unwrap_or(0), room);
        let visible = self.rows.iter().skip(first).take(room);
        visible
            .filter(|row| row.session().is_some())
            .map(|row| row.name().to_string())
            .collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.rows.iter().map(|row| row.name().to_string()).collect()
    }

    pub fn apply_diffs(&mut self, diffs: &HashMap<String, DiffStat>) {
        let rows = std::mem::take(&mut self.rows);
        self.rows = rows
            .into_iter()
            .map(|row| match diffs.get(row.name()) {
                Some(diff) => row.with_diff(*diff),
                None => row,
            })
            .collect();
    }

    pub fn apply_pull_requests(&mut self, pull_requests: &HashMap<String, PullRequest>) {
        let rows = std::mem::take(&mut self.rows);
        self.rows = rows
            .into_iter()
            .map(|row| match pull_requests.get(row.name()) {
                Some(pull_request) => row.with_pull_request(*pull_request),
                None => row,
            })
            .collect();
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
        self.select_index(current.saturating_add_signed(offset));
    }

    pub fn select_first(&mut self) {
        self.select_index(0);
    }

    pub fn select_last(&mut self) {
        self.select_index(self.last_index());
    }

    fn selected_index(&self) -> Option<usize> {
        let selected = self.selection.as_deref()?;
        self.rows.iter().position(|row| row.name() == selected)
    }

    fn select_index(&mut self, index: usize) {
        if let Some(row) = self.rows.get(index.min(self.last_index())) {
            self.selection = Some(row.name().to_string());
        }
    }

    fn last_index(&self) -> usize {
        self.rows.len().saturating_sub(1)
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

        fn three_waiting_rows() -> SessionList {
            let mut list = SessionList::default();
            list.show(vec![
                Self::row("dark-mode", SessionState::Waiting),
                Self::row("fix-auth", SessionState::Waiting),
                Self::row("spike-tui", SessionState::Waiting),
            ]);
            list
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
    fn only_visible_managed_rows_are_offered_for_diffs() {
        let stranger = ServiceFixture::session("mystery");
        let unmanaged = SessionSummary::unmanaged(crate::domain::UnmanagedSession::new(
            "mystery",
            ServiceFixture::waiting(&stranger).without_metadata(),
        ));
        let mut list = SessionList::default();
        list.show(vec![
            ListFixture::row("a-one", SessionState::Waiting),
            ListFixture::row("b-two", SessionState::Waiting),
            ListFixture::row("c-three", SessionState::Waiting),
            unmanaged,
        ]);
        list.move_selection(3);

        assert_eq!(list.visible_managed(2), ["c-three"]);
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

    #[test]
    fn select_last_picks_the_bottom_row() {
        let mut list = ListFixture::three_waiting_rows();

        list.select_last();

        assert_eq!(list.selected_name(), Some("spike-tui"));
    }

    #[test]
    fn select_first_picks_the_top_row_after_a_move() {
        let mut list = ListFixture::three_waiting_rows();
        list.move_selection(2);

        list.select_first();

        assert_eq!(list.selected_name(), Some("dark-mode"));
    }

    #[test]
    fn jumping_on_an_empty_list_selects_nothing() {
        let mut list = SessionList::default();

        list.select_last();
        list.select_first();

        assert_eq!(list.selected_name(), None);
    }
}

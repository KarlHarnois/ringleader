use std::collections::{HashMap, HashSet};
use std::ops::Range;

use super::list_selection::ListSelection;
use super::repo_heading::RepoHeading;
use super::session_table::SessionTable;
use super::session_tree::{SessionTree, TreeLine};
use super::viewport::Viewport;
use crate::domain::{DiffStat, PullRequest, RepoGroup, SessionSummary};
use crate::ui::theme::Theme;

#[derive(Debug, Default)]
pub struct SessionList {
    rows: Vec<SessionSummary>,
    selection: Option<ListSelection>,
    visual_anchor: Option<ListSelection>,
    folded: HashSet<RepoGroup>,
    viewport: Viewport,
}

impl SessionList {
    pub fn rows(&self) -> &[SessionSummary] {
        &self.rows
    }

    pub fn table(&self, tick: u64, theme: Theme) -> SessionTable<'_> {
        SessionTable::new(self.tree(), self.selection.as_ref(), tick, theme)
            .with_viewport(self.viewport)
            .with_marked(self.visual_selections())
    }

    pub fn in_visual_mode(&self) -> bool {
        self.visual_anchor.is_some()
    }

    pub fn toggle_visual(&mut self) {
        self.visual_anchor = match self.visual_anchor.take() {
            Some(_) => None,
            None => self.selection.clone(),
        };
    }

    pub fn leave_visual(&mut self) {
        self.visual_anchor = None;
    }

    pub fn visual_names(&self) -> Vec<String> {
        let tree = self.tree();
        tree.lines()[self.visual_lines(&tree)]
            .iter()
            .flat_map(TreeLine::covered_rows)
            .map(|row| row.name().to_string())
            .collect()
    }

    pub fn selected_name(&self) -> Option<&str> {
        self.selected().map(SessionSummary::name)
    }

    pub fn selected_heading(&self) -> Option<&RepoGroup> {
        match self.selection.as_ref()? {
            ListSelection::Repo(group) => Some(group),
            ListSelection::Session(_) => None,
        }
    }

    pub fn visible_managed(&self) -> Vec<String> {
        let tree = self.tree();
        let visible = tree.visible(self.selection.as_ref(), self.viewport);
        visible
            .filter_map(TreeLine::row)
            .filter(|row| row.session().is_some())
            .map(|row| row.name().to_string())
            .collect()
    }

    pub fn folded(&self) -> &HashSet<RepoGroup> {
        &self.folded
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
                Some(pull_request) => row.with_pull_request(pull_request.clone()),
                None => row,
            })
            .collect();
    }

    pub fn row(&self, name: &str) -> Option<&SessionSummary> {
        self.rows.iter().find(|row| row.name() == name)
    }

    pub fn selected(&self) -> Option<&SessionSummary> {
        match self.selection.as_ref()? {
            ListSelection::Session(name) => self.row(name),
            ListSelection::Repo(_) => None,
        }
    }

    pub fn resize(&mut self, room: usize) {
        self.viewport = self.viewport.with_room(room);
        self.scroll_to_selection();
    }

    pub fn show(&mut self, mut rows: Vec<SessionSummary>) {
        SessionTree::sort(&mut rows);
        self.rows = rows;
        self.forget_folds_without_a_heading();
        self.selection = self.repaired_selection();
        self.forget_a_hidden_anchor();
        self.scroll_to_selection();
    }

    pub fn select(&mut self, name: &str) {
        let Some(group) = self.row(name).map(RepoGroup::of) else {
            return;
        };
        self.folded.remove(&group);
        self.move_selection_to(ListSelection::Session(name.to_string()));
    }

    pub fn move_selection(&mut self, offset: isize) {
        if let Some(selection) = self.selection_moved_by(offset) {
            self.move_selection_to(selection);
        }
    }

    pub fn select_first(&mut self) {
        self.select_line(0);
    }

    pub fn select_last(&mut self) {
        self.select_line(self.line_count().saturating_sub(1));
    }

    pub fn select_high(&mut self) {
        self.select_shown_line(self.viewport.high(self.line_count()));
    }

    pub fn select_middle(&mut self) {
        self.select_shown_line(self.viewport.middle(self.line_count()));
    }

    pub fn select_low(&mut self) {
        self.select_shown_line(self.viewport.low(self.line_count()));
    }

    pub fn toggle_fold(&mut self) {
        let Some(ListSelection::Repo(group)) = &self.selection else {
            return;
        };
        if !self.folded.remove(group) {
            self.folded.insert(group.clone());
        }
        self.forget_a_hidden_anchor();
        self.scroll_to_selection();
    }

    pub fn restore_folds(&mut self, folded: HashSet<RepoGroup>) {
        self.folded = folded;
    }

    pub fn jump_to_repo(&mut self, number: usize) {
        if let Some(target) = self.jump_target(number) {
            self.move_selection_to(target);
        }
    }

    fn tree(&self) -> SessionTree<'_> {
        SessionTree::new(&self.rows, &self.folded)
    }

    fn visual_selections(&self) -> Vec<ListSelection> {
        let tree = self.tree();
        tree.lines()[self.visual_lines(&tree)]
            .iter()
            .map(TreeLine::selection)
            .collect()
    }

    fn visual_lines(&self, tree: &SessionTree) -> Range<usize> {
        let Some(anchor) = self.visual_anchor.as_ref() else {
            return 0..0;
        };
        let anchor_line = tree.selected_line(Some(anchor));
        let cursor_line = tree.selected_line(self.selection.as_ref());
        anchor_line.min(cursor_line)..anchor_line.max(cursor_line) + 1
    }

    fn forget_a_hidden_anchor(&mut self) {
        if !self.anchor_is_shown() {
            self.visual_anchor = None;
        }
    }

    fn anchor_is_shown(&self) -> bool {
        let Some(anchor) = self.visual_anchor.as_ref() else {
            return true;
        };
        self.tree().position_of(anchor).is_some()
    }

    fn line_count(&self) -> usize {
        self.tree().lines().len()
    }

    fn select_shown_line(&mut self, line: Option<usize>) {
        if let Some(line) = line {
            self.select_line(line);
        }
    }

    fn select_line(&mut self, line: usize) {
        let selection = self.tree().lines().get(line).map(TreeLine::selection);
        if let Some(selection) = selection {
            self.move_selection_to(selection);
        }
    }

    fn move_selection_to(&mut self, selection: ListSelection) {
        self.selection = Some(selection);
        self.scroll_to_selection();
    }

    fn scroll_to_selection(&mut self) {
        let tree = self.tree();
        let selected = tree.selected_line(self.selection.as_ref());
        let line_count = tree.lines().len();
        self.viewport = self.viewport.scrolled_to(selected, line_count);
    }

    fn forget_folds_without_a_heading(&mut self) {
        let tree = self.tree();
        let headed: HashSet<RepoGroup> = tree
            .headings()
            .map(|heading| heading.group().clone())
            .collect();
        self.folded.retain(|group| headed.contains(group));
    }

    fn repaired_selection(&self) -> Option<ListSelection> {
        let tree = self.tree();
        let first_line = || tree.lines().first().map(TreeLine::selection);
        let Some(selection) = &self.selection else {
            return first_line();
        };
        if tree.position_of(selection).is_some() {
            return Some(selection.clone());
        }
        self.folded_heading_of(selection).or_else(first_line)
    }

    fn folded_heading_of(&self, selection: &ListSelection) -> Option<ListSelection> {
        let ListSelection::Session(name) = selection else {
            return None;
        };
        let group = self.row(name).map(RepoGroup::of)?;
        Some(ListSelection::Repo(group))
    }

    fn jump_target(&self, number: usize) -> Option<ListSelection> {
        self.tree().heading(number).map(RepoHeading::jump_target)
    }

    fn selection_moved_by(&self, offset: isize) -> Option<ListSelection> {
        let tree = self.tree();
        let current = tree.selected_line(self.selection.as_ref());
        let last = tree.lines().len().saturating_sub(1);
        let target = current.saturating_add_signed(offset).min(last);
        tree.lines().get(target).map(TreeLine::selection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::SessionState;
    use crate::test_support::ServiceFixture;

    struct ListFixture;

    impl ListFixture {
        const ROOM_FOR_EVERY_LINE: usize = 10;
        const ROOM_FOR_THREE_LINES: usize = 3;

        fn row(name: &str, state: SessionState) -> SessionSummary {
            SessionSummary::managed(ServiceFixture::session(name), state, false)
        }

        fn two_repos() -> Vec<SessionSummary> {
            vec![
                ServiceFixture::summary_in("a-one", "/repos/alpha", SessionState::Waiting),
                ServiceFixture::summary_in("b-two", "/repos/alpha", SessionState::Waiting),
                ServiceFixture::summary_in("c-three", "/repos/beta", SessionState::Waiting),
            ]
        }

        fn showing(rows: Vec<SessionSummary>) -> SessionList {
            let mut list = SessionList::default();
            list.resize(Self::ROOM_FOR_EVERY_LINE);
            list.show(rows);
            list
        }

        fn six_rows_in_three_lines() -> SessionList {
            let names = ["a-one", "b-two", "c-three", "d-four", "e-five", "f-six"];
            let rows = names
                .iter()
                .map(|name| Self::row(name, SessionState::Waiting))
                .collect();
            let mut list = Self::showing(rows);
            list.resize(Self::ROOM_FOR_THREE_LINES);
            list
        }

        fn alpha_folded() -> HashSet<RepoGroup> {
            HashSet::from([ServiceFixture::repo_group("/repos/alpha")])
        }

        fn restored_alpha_fold() -> SessionList {
            let mut list = SessionList::default();
            list.resize(Self::ROOM_FOR_EVERY_LINE);
            list.restore_folds(Self::alpha_folded());
            list
        }

        fn names(list: &SessionList) -> Vec<&str> {
            list.rows().iter().map(SessionSummary::name).collect()
        }

        fn three_waiting_rows() -> SessionList {
            Self::showing(vec![
                Self::row("dark-mode", SessionState::Waiting),
                Self::row("fix-auth", SessionState::Waiting),
                Self::row("spike-tui", SessionState::Waiting),
            ])
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
    fn rows_sort_by_repo_before_state() {
        let mut list = SessionList::default();

        list.show(vec![
            ServiceFixture::summary_in("zeta", "/repos/beta", SessionState::Working),
            ServiceFixture::summary_in("alpha-one", "/repos/alpha", SessionState::Stopped),
            ServiceFixture::summary_in("alpha-two", "/repos/alpha", SessionState::Working),
        ]);

        assert_eq!(
            ListFixture::names(&list),
            ["alpha-two", "alpha-one", "zeta"]
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
            ServiceFixture::waiting(&stranger),
        ));
        let mut list = ListFixture::showing(vec![
            ListFixture::row("a-one", SessionState::Waiting),
            ListFixture::row("b-two", SessionState::Waiting),
            ListFixture::row("c-three", SessionState::Waiting),
            unmanaged,
        ]);
        list.resize(2);
        list.move_selection(3);

        assert_eq!(list.visible_managed(), ["c-three"]);
    }

    #[test]
    fn heading_lines_count_against_the_room_for_diffs() {
        let mut list = ListFixture::showing(ListFixture::two_repos());
        list.resize(2);
        list.select("c-three");

        assert_eq!(list.visible_managed(), ["c-three"]);
    }

    #[test]
    fn visual_mode_covers_every_session_from_the_anchor_to_the_cursor() {
        let mut list = ListFixture::six_rows_in_three_lines();
        list.move_selection(1);

        list.toggle_visual();
        list.move_selection(2);

        assert_eq!(list.visual_names(), ["b-two", "c-three", "d-four"]);
    }

    #[test]
    fn visual_mode_covers_a_range_that_grows_upward() {
        let mut list = ListFixture::six_rows_in_three_lines();
        list.select_last();

        list.toggle_visual();
        list.move_selection(-1);

        assert_eq!(list.visual_names(), ["e-five", "f-six"]);
    }

    #[test]
    fn a_folded_repo_in_the_range_brings_its_hidden_sessions_as_a_vim_fold_does() {
        let mut list = ListFixture::showing(ListFixture::two_repos());
        list.toggle_fold();

        list.toggle_visual();
        list.select_last();

        assert_eq!(list.visual_names(), ["a-one", "b-two", "c-three"]);
    }

    #[test]
    fn toggling_twice_leaves_visual_mode() {
        let mut list = ListFixture::six_rows_in_three_lines();

        list.toggle_visual();
        list.toggle_visual();

        assert!(!list.in_visual_mode());
        assert!(list.visual_names().is_empty());
    }

    #[test]
    fn visual_mode_ends_when_its_anchor_leaves_the_list() {
        let mut list = ListFixture::showing(vec![
            ListFixture::row("a-one", SessionState::Waiting),
            ListFixture::row("b-two", SessionState::Waiting),
        ]);
        list.toggle_visual();

        list.show(vec![ListFixture::row("b-two", SessionState::Waiting)]);

        assert!(!list.in_visual_mode());
    }

    #[test]
    fn folding_the_repo_of_the_anchor_ends_visual_mode() {
        let mut list = ListFixture::showing(ListFixture::two_repos());
        list.move_selection(1);
        list.toggle_visual();
        list.move_selection(-1);

        list.toggle_fold();

        assert!(!list.in_visual_mode());
    }

    #[test]
    fn folded_sessions_are_not_offered_for_diffs() {
        let mut list = ListFixture::showing(ListFixture::two_repos());

        list.toggle_fold();

        assert_eq!(list.visible_managed(), ["c-three"]);
    }

    #[test]
    fn a_grouped_list_starts_on_its_first_heading() {
        let list = ListFixture::showing(ListFixture::two_repos());

        assert!(list.selected_heading().is_some());
        assert_eq!(list.selected(), None);
    }

    #[test]
    fn moving_down_from_a_repo_s_last_session_reaches_the_next_heading() {
        let mut list = ListFixture::showing(ListFixture::two_repos());
        list.select("b-two");

        list.move_selection(1);
        let on_heading = list.selected_heading().is_some();
        list.move_selection(1);

        assert!(on_heading);
        assert_eq!(list.selected_name(), Some("c-three"));
    }

    #[test]
    fn folding_does_nothing_on_a_session() {
        let mut list = ListFixture::showing(ListFixture::two_repos());
        list.select("a-one");

        list.toggle_fold();

        assert_eq!(list.visible_managed(), ["a-one", "b-two", "c-three"]);
    }

    #[test]
    fn folding_twice_unfolds_the_repo() {
        let mut list = ListFixture::showing(ListFixture::two_repos());

        list.toggle_fold();
        list.toggle_fold();

        assert_eq!(list.visible_managed(), ["a-one", "b-two", "c-three"]);
    }

    #[test]
    fn a_selected_session_that_reappears_in_a_folded_repo_gives_way_to_its_heading() {
        let mut list = ListFixture::showing(ListFixture::two_repos());
        list.select("c-three");
        list.move_selection(-1);
        list.toggle_fold();
        list.select("a-one");

        list.show(vec![
            ServiceFixture::summary_in("b-two", "/repos/alpha", SessionState::Waiting),
            ServiceFixture::summary_in("a-one", "/repos/beta", SessionState::Waiting),
            ServiceFixture::summary_in("c-three", "/repos/beta", SessionState::Waiting),
        ]);
        let on_heading = list.selected_heading().is_some();
        list.toggle_fold();

        assert!(on_heading);
        assert_eq!(list.visible_managed(), ["b-two", "a-one", "c-three"]);
    }

    #[test]
    fn a_fold_is_forgotten_once_its_repo_loses_its_heading() {
        let mut list = ListFixture::showing(ListFixture::two_repos());
        list.select("c-three");
        list.move_selection(-1);
        list.toggle_fold();

        list.show(ListFixture::two_repos()[2..].to_vec());
        list.show(ListFixture::two_repos());

        assert_eq!(list.visible_managed(), ["a-one", "b-two", "c-three"]);
    }

    #[test]
    fn a_restored_fold_folds_its_repo_once_the_rows_arrive() {
        let mut list = ListFixture::restored_alpha_fold();

        list.show(ListFixture::two_repos());

        assert_eq!(list.visible_managed(), ["c-three"]);
    }

    #[test]
    fn a_restored_fold_without_a_heading_is_forgotten_by_the_first_rows() {
        let mut list = ListFixture::restored_alpha_fold();

        list.show(ListFixture::two_repos()[2..].to_vec());

        assert!(list.folded().is_empty());
    }

    #[test]
    fn folded_names_the_repo_of_each_folded_heading() {
        let mut list = ListFixture::showing(ListFixture::two_repos());

        list.toggle_fold();

        assert_eq!(list.folded(), &ListFixture::alpha_folded());
    }

    #[test]
    fn a_vanished_heading_falls_back_to_the_first_line() {
        let mut list = ListFixture::showing(ListFixture::two_repos());
        list.select("c-three");
        list.move_selection(-1);

        list.show(ListFixture::two_repos()[..2].to_vec());

        assert_eq!(list.selected_name(), Some("a-one"));
    }

    #[test]
    fn jumping_to_a_repo_selects_its_first_session() {
        let mut list = ListFixture::showing(ListFixture::two_repos());

        list.jump_to_repo(2);

        assert_eq!(list.selected_name(), Some("c-three"));
    }

    #[test]
    fn jumping_to_a_folded_repo_selects_its_heading() {
        let mut list = ListFixture::showing(ListFixture::two_repos());
        list.toggle_fold();
        list.select("c-three");

        list.jump_to_repo(1);
        list.toggle_fold();

        assert_eq!(list.visible_managed(), ["a-one", "b-two", "c-three"]);
    }

    #[test]
    fn jumping_past_the_last_repo_keeps_the_selection() {
        let mut list = ListFixture::showing(ListFixture::two_repos());
        list.select("b-two");

        list.jump_to_repo(3);

        assert_eq!(list.selected_name(), Some("b-two"));
    }

    #[test]
    fn a_single_repo_has_no_repo_to_jump_to() {
        let mut list = ListFixture::showing(ListFixture::two_repos()[..2].to_vec());
        list.select("b-two");

        list.jump_to_repo(1);

        assert_eq!(list.selected_name(), Some("b-two"));
    }

    #[test]
    fn selecting_a_session_unfolds_its_repo() {
        let mut list = ListFixture::showing(ListFixture::two_repos());
        list.toggle_fold();

        list.select("b-two");

        assert_eq!(list.selected_name(), Some("b-two"));
        assert_eq!(list.visible_managed(), ["a-one", "b-two", "c-three"]);
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
    fn select_first_picks_the_first_heading_of_a_grouped_list() {
        let mut list = ListFixture::showing(ListFixture::two_repos());
        list.select("c-three");

        list.select_first();
        list.toggle_fold();

        assert_eq!(list.visible_managed(), ["c-three"]);
    }

    #[test]
    fn select_last_picks_the_last_session_of_a_grouped_list() {
        let mut list = ListFixture::showing(ListFixture::two_repos());

        list.select_last();

        assert_eq!(list.selected_name(), Some("c-three"));
    }

    #[test]
    fn jumping_on_an_empty_list_selects_nothing() {
        let mut list = SessionList::default();

        list.select_last();
        list.select_first();
        list.select_high();
        list.select_middle();
        list.select_low();

        assert_eq!(list.selected_name(), None);
    }

    #[test]
    fn select_low_picks_the_bottom_row_on_screen() {
        let mut list = ListFixture::six_rows_in_three_lines();

        list.select_low();

        assert_eq!(list.selected_name(), Some("c-three"));
    }

    #[test]
    fn select_high_picks_the_top_row_on_screen_without_scrolling() {
        let mut list = ListFixture::six_rows_in_three_lines();
        list.select_last();

        list.select_high();

        assert_eq!(list.selected_name(), Some("d-four"));
        assert_eq!(list.visible_managed(), ["d-four", "e-five", "f-six"]);
    }

    #[test]
    fn select_middle_picks_the_middle_row_on_screen() {
        let mut list = ListFixture::six_rows_in_three_lines();
        list.select_last();

        list.select_middle();

        assert_eq!(list.selected_name(), Some("e-five"));
    }

    #[test]
    fn select_middle_picks_the_middle_of_a_list_shorter_than_the_screen() {
        let mut list = ListFixture::three_waiting_rows();

        list.select_middle();

        assert_eq!(list.selected_name(), Some("fix-auth"));
    }

    #[test]
    fn moving_up_inside_a_scrolled_view_keeps_the_view() {
        let mut list = ListFixture::six_rows_in_three_lines();
        list.select_last();

        list.move_selection(-1);

        assert_eq!(list.selected_name(), Some("e-five"));
        assert_eq!(list.visible_managed(), ["d-four", "e-five", "f-six"]);
    }
}

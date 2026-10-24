use std::collections::HashSet;
use std::iter::once;

use super::list_selection::ListSelection;
use super::repo_heading::RepoHeading;
use super::viewport::Viewport;
use crate::domain::{RepoGroup, SessionFilter, SessionSummary};

pub struct SessionTree<'a> {
    rows: &'a [SessionSummary],
    lines: Vec<TreeLine<'a>>,
    filter: SessionFilter,
}

impl<'a> SessionTree<'a> {
    pub fn new(
        rows: &'a [SessionSummary],
        folded: &HashSet<RepoGroup>,
        filter: SessionFilter,
    ) -> Self {
        let lines = if Self::groups_visible_rows_by_repo(rows, filter) {
            Self::grouped_lines(rows, folded, filter)
        } else {
            Self::flat_lines(rows, filter)
        };
        Self {
            rows,
            lines,
            filter,
        }
    }

    pub fn groups_by_repo(rows: &[SessionSummary]) -> bool {
        rows.chunk_by(Self::share_a_group).nth(1).is_some()
    }

    pub fn sort(rows: &mut [SessionSummary]) {
        rows.sort_by_cached_key(|row| (RepoGroup::of(row), row.state(), row.name().to_string()));
    }

    pub fn rows(&self) -> impl Iterator<Item = &'a SessionSummary> + '_ {
        let filter = self.filter;
        self.rows.iter().filter(move |row| filter.allows(row))
    }

    pub fn filter(&self) -> SessionFilter {
        self.filter
    }

    pub fn lines(&self) -> &[TreeLine<'a>] {
        &self.lines
    }

    pub fn is_grouped(&self) -> bool {
        self.lines.iter().any(|line| line.heading().is_some())
    }

    pub fn heading(&self, number: usize) -> Option<&RepoHeading<'a>> {
        self.headings().find(|heading| heading.number() == number)
    }

    pub fn headings(&self) -> impl Iterator<Item = &RepoHeading<'a>> {
        self.lines.iter().filter_map(TreeLine::heading)
    }

    pub fn visible(
        &self,
        selection: Option<&ListSelection>,
        viewport: Viewport,
    ) -> impl Iterator<Item = &TreeLine<'a>> {
        let line_count = self.lines.len();
        let scrolled = viewport.scrolled_to(self.selected_line(selection), line_count);
        self.lines[scrolled.lines(line_count)].iter()
    }

    pub fn selected_line(&self, selection: Option<&ListSelection>) -> usize {
        selection
            .and_then(|selection| self.position_of(selection))
            .unwrap_or(0)
    }

    pub fn position_of(&self, selection: &ListSelection) -> Option<usize> {
        self.lines
            .iter()
            .position(|line| line.selection() == *selection)
    }

    fn groups_visible_rows_by_repo(rows: &[SessionSummary], filter: SessionFilter) -> bool {
        let mut groups = rows
            .iter()
            .filter(|row| filter.allows(row))
            .map(RepoGroup::of);
        let Some(first) = groups.next() else {
            return false;
        };
        groups.any(|group| group != first)
    }

    fn share_a_group(first: &SessionSummary, second: &SessionSummary) -> bool {
        RepoGroup::of(first) == RepoGroup::of(second)
    }

    fn flat_lines(rows: &'a [SessionSummary], filter: SessionFilter) -> Vec<TreeLine<'a>> {
        rows.iter()
            .filter(|row| filter.allows(row))
            .map(|row| TreeLine::session(row, TreeBranch::Flat))
            .collect()
    }

    fn grouped_lines(
        rows: &'a [SessionSummary],
        folded: &HashSet<RepoGroup>,
        filter: SessionFilter,
    ) -> Vec<TreeLine<'a>> {
        let mut next_number = 1;
        let headings = rows.chunk_by(Self::share_a_group).filter_map(|rows| {
            let heading = RepoHeading::visible(next_number, rows, filter)?;
            next_number += 1;
            Some(heading.folded_among(folded))
        });
        headings.flat_map(Self::group_lines).collect()
    }

    fn group_lines(heading: RepoHeading<'a>) -> Vec<TreeLine<'a>> {
        let branches = Self::branch_lines(heading.shown_rows());
        once(TreeLine::Heading(heading)).chain(branches).collect()
    }

    fn branch_lines(rows: impl Iterator<Item = &'a SessionSummary>) -> Vec<TreeLine<'a>> {
        let mut rows = rows.peekable();
        let mut lines = Vec::new();
        while let Some(row) = rows.next() {
            let branch = if rows.peek().is_some() {
                TreeBranch::Middle
            } else {
                TreeBranch::Last
            };
            lines.push(TreeLine::session(row, branch));
        }
        lines
    }
}

pub enum TreeLine<'a> {
    Heading(RepoHeading<'a>),
    Session {
        row: &'a SessionSummary,
        branch: TreeBranch,
    },
}

impl<'a> TreeLine<'a> {
    fn session(row: &'a SessionSummary, branch: TreeBranch) -> Self {
        Self::Session { row, branch }
    }

    pub fn row(&self) -> Option<&'a SessionSummary> {
        match self {
            Self::Session { row, .. } => Some(row),
            Self::Heading(_) => None,
        }
    }

    pub fn selection(&self) -> ListSelection {
        match self {
            Self::Session { row, .. } => ListSelection::Session(row.name().to_string()),
            Self::Heading(heading) => ListSelection::Repo(heading.group().clone()),
        }
    }

    fn heading(&self) -> Option<&RepoHeading<'a>> {
        match self {
            Self::Heading(heading) => Some(heading),
            Self::Session { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeBranch {
    Flat,
    Middle,
    Last,
}

impl TreeBranch {
    pub const WIDTH: usize = 5;
    const MIDDLE: &'static str = "  ├─ ";
    const LAST: &'static str = "  └─ ";

    pub fn prefix(self) -> &'static str {
        match self {
            Self::Flat => "",
            Self::Middle => Self::MIDDLE,
            Self::Last => Self::LAST,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::SessionState;
    use crate::test_support::ServiceFixture;

    struct TreeFixture;

    impl TreeFixture {
        fn row(name: &str, repo: &str) -> SessionSummary {
            ServiceFixture::summary_in(name, repo, SessionState::Waiting)
        }

        fn favorite(name: &str, repo: &str) -> SessionSummary {
            let session =
                ServiceFixture::session_at(name, repo, ServiceFixture::CREATED_AT).favorited();
            SessionSummary::managed(session, SessionState::Waiting, false)
        }

        fn two_repos() -> Vec<SessionSummary> {
            vec![
                Self::row("dark-mode", "/repos/alpha"),
                Self::row("fix-auth", "/repos/alpha"),
                Self::row("docs", "/repos/beta"),
            ]
        }

        fn shapes(tree: &SessionTree) -> Vec<String> {
            tree.lines().iter().map(Self::shape).collect()
        }

        fn shape(line: &TreeLine) -> String {
            match line {
                TreeLine::Heading(_) => "heading".to_string(),
                TreeLine::Session { row, branch } => format!("{:?} {}", branch, row.name()),
            }
        }
    }

    #[test]
    fn one_repo_gives_flat_lines_without_headings() {
        let rows = [
            TreeFixture::row("dark-mode", "/repos/alpha"),
            TreeFixture::row("fix-auth", "/repos/alpha"),
        ];

        let tree = SessionTree::new(&rows, &HashSet::new(), SessionFilter::All);

        assert_eq!(
            TreeFixture::shapes(&tree),
            ["Flat dark-mode", "Flat fix-auth"]
        );
        assert!(!tree.is_grouped());
    }

    #[test]
    fn two_repos_give_a_heading_then_middle_and_last_branches() {
        let rows = TreeFixture::two_repos();

        let tree = SessionTree::new(&rows, &HashSet::new(), SessionFilter::All);

        assert_eq!(
            TreeFixture::shapes(&tree),
            [
                "heading",
                "Middle dark-mode",
                "Last fix-auth",
                "heading",
                "Last docs"
            ]
        );
        assert!(tree.is_grouped());
    }

    #[test]
    fn a_folded_group_yields_only_its_heading() {
        let rows = TreeFixture::two_repos();
        let folded = HashSet::from([RepoGroup::of(&rows[0])]);

        let tree = SessionTree::new(&rows, &folded, SessionFilter::All);

        assert_eq!(
            TreeFixture::shapes(&tree),
            ["heading", "heading", "Last docs"]
        );
    }

    #[test]
    fn a_single_repo_ignores_its_fold() {
        let rows = [TreeFixture::row("dark-mode", "/repos/alpha")];
        let folded = HashSet::from([RepoGroup::of(&rows[0])]);

        let tree = SessionTree::new(&rows, &folded, SessionFilter::All);

        assert_eq!(TreeFixture::shapes(&tree), ["Flat dark-mode"]);
    }

    #[test]
    fn the_favorites_filter_keeps_only_favorites() {
        let rows = vec![
            TreeFixture::favorite("fix-auth", "/repos/alpha"),
            TreeFixture::row("dark-mode", "/repos/alpha"),
            TreeFixture::row("docs", "/repos/beta"),
        ];

        let tree = SessionTree::new(&rows, &HashSet::new(), SessionFilter::Favorites);

        assert_eq!(TreeFixture::shapes(&tree), ["Flat fix-auth"]);
        assert_eq!(tree.rows().count(), 1);
    }

    #[test]
    fn a_filtered_single_repo_is_flat() {
        let rows = vec![
            TreeFixture::favorite("fix-auth", "/repos/alpha"),
            TreeFixture::row("dark-mode", "/repos/alpha"),
            TreeFixture::row("docs", "/repos/beta"),
        ];

        let tree = SessionTree::new(&rows, &HashSet::new(), SessionFilter::Favorites);

        assert!(!tree.is_grouped());
    }

    #[test]
    fn visible_headings_are_numbered_without_gaps() {
        let rows = vec![
            TreeFixture::row("alpha-one", "/repos/alpha"),
            TreeFixture::favorite("beta-one", "/repos/beta"),
            TreeFixture::favorite("gamma-one", "/repos/gamma"),
        ];

        let tree = SessionTree::new(&rows, &HashSet::new(), SessionFilter::Favorites);
        let numbers: Vec<usize> = tree.headings().map(RepoHeading::number).collect();

        assert_eq!(numbers, [1, 2]);
        assert_eq!(tree.heading(1).unwrap().group(), &RepoGroup::of(&rows[1]));
    }

    #[test]
    fn the_selection_stays_on_screen_when_headings_use_up_lines() {
        let rows = TreeFixture::two_repos();
        let tree = SessionTree::new(&rows, &HashSet::new(), SessionFilter::All);
        let docs = ListSelection::Session("docs".to_string());

        let visible: Vec<String> = tree
            .visible(Some(&docs), Viewport::default().with_room(2))
            .map(TreeFixture::shape)
            .collect();

        assert_eq!(visible, ["heading", "Last docs"]);
    }

    #[test]
    fn every_branch_prefix_takes_the_branch_width() {
        for branch in [TreeBranch::Middle, TreeBranch::Last] {
            assert_eq!(branch.prefix().chars().count(), TreeBranch::WIDTH);
        }
    }
}

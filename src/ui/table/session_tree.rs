use std::collections::HashSet;
use std::iter::once;

use super::list_selection::ListSelection;
use super::repo_heading::RepoHeading;
use super::viewport::Viewport;
use crate::domain::{RepoGroup, SessionSummary};

pub struct SessionTree<'a> {
    rows: &'a [SessionSummary],
    lines: Vec<TreeLine<'a>>,
}

impl<'a> SessionTree<'a> {
    pub fn new(rows: &'a [SessionSummary], folded: &HashSet<RepoGroup>) -> Self {
        let groups: Vec<&'a [SessionSummary]> = rows.chunk_by(Self::share_a_group).collect();
        let lines = match groups.len() {
            0 | 1 => Self::flat_lines(rows),
            _ => Self::grouped_lines(groups, folded),
        };
        Self { rows, lines }
    }

    pub fn sort(rows: &mut [SessionSummary]) {
        rows.sort_by_cached_key(|row| (RepoGroup::of(row), row.state(), row.name().to_string()));
    }

    pub fn rows(&self) -> &'a [SessionSummary] {
        self.rows
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

    fn share_a_group(first: &SessionSummary, second: &SessionSummary) -> bool {
        RepoGroup::of(first) == RepoGroup::of(second)
    }

    fn flat_lines(rows: &'a [SessionSummary]) -> Vec<TreeLine<'a>> {
        let flat = |row| TreeLine::session(row, TreeBranch::Flat);
        rows.iter().map(flat).collect()
    }

    fn grouped_lines(
        groups: Vec<&'a [SessionSummary]>,
        folded: &HashSet<RepoGroup>,
    ) -> Vec<TreeLine<'a>> {
        let numbered = groups.into_iter().zip(1..);
        let headings =
            numbered.map(|(rows, number)| RepoHeading::new(number, rows).folded_among(folded));
        headings.flat_map(Self::group_lines).collect()
    }

    fn group_lines(heading: RepoHeading<'a>) -> Vec<TreeLine<'a>> {
        let branches = Self::branch_lines(heading.shown_rows());
        once(TreeLine::Heading(heading)).chain(branches).collect()
    }

    fn branch_lines(rows: &'a [SessionSummary]) -> Vec<TreeLine<'a>> {
        let Some((last, middle)) = rows.split_last() else {
            return Vec::new();
        };
        let middle = middle
            .iter()
            .map(|row| TreeLine::session(row, TreeBranch::Middle));
        let last = TreeLine::session(last, TreeBranch::Last);
        middle.chain(once(last)).collect()
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

        let tree = SessionTree::new(&rows, &HashSet::new());

        assert_eq!(
            TreeFixture::shapes(&tree),
            ["Flat dark-mode", "Flat fix-auth"]
        );
        assert!(!tree.is_grouped());
    }

    #[test]
    fn two_repos_give_a_heading_then_middle_and_last_branches() {
        let rows = TreeFixture::two_repos();

        let tree = SessionTree::new(&rows, &HashSet::new());

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

        let tree = SessionTree::new(&rows, &folded);

        assert_eq!(
            TreeFixture::shapes(&tree),
            ["heading", "heading", "Last docs"]
        );
    }

    #[test]
    fn a_single_repo_ignores_its_fold() {
        let rows = [TreeFixture::row("dark-mode", "/repos/alpha")];
        let folded = HashSet::from([RepoGroup::of(&rows[0])]);

        let tree = SessionTree::new(&rows, &folded);

        assert_eq!(TreeFixture::shapes(&tree), ["Flat dark-mode"]);
    }

    #[test]
    fn the_selection_stays_on_screen_when_headings_use_up_lines() {
        let rows = TreeFixture::two_repos();
        let tree = SessionTree::new(&rows, &HashSet::new());
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

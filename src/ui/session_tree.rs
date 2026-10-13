use std::iter::once;

use super::repo_group::RepoGroup;
use super::repo_heading::RepoHeading;
use crate::domain::SessionSummary;

pub struct SessionTree<'a> {
    rows: &'a [SessionSummary],
    lines: Vec<TreeLine<'a>>,
}

impl<'a> SessionTree<'a> {
    pub fn new(rows: &'a [SessionSummary]) -> Self {
        let groups: Vec<&'a [SessionSummary]> = rows.chunk_by(Self::share_a_group).collect();
        let lines = match groups.len() {
            0 | 1 => Self::flat_lines(rows),
            _ => Self::grouped_lines(groups),
        };
        Self { rows, lines }
    }

    pub fn rows(&self) -> &'a [SessionSummary] {
        self.rows
    }

    #[cfg(test)]
    pub fn lines(&self) -> &[TreeLine<'a>] {
        &self.lines
    }

    pub fn is_grouped(&self) -> bool {
        self.lines.iter().any(TreeLine::is_heading)
    }

    pub fn visible(
        &self,
        selection: Option<&str>,
        room: usize,
    ) -> impl Iterator<Item = &TreeLine<'a>> {
        let selected_line = selection
            .and_then(|name| self.position_of(name))
            .unwrap_or(0);
        let first = (selected_line + 1).saturating_sub(room);
        self.lines.iter().skip(first).take(room)
    }

    pub fn position_of(&self, name: &str) -> Option<usize> {
        self.lines
            .iter()
            .position(|line| line.row().is_some_and(|row| row.name() == name))
    }

    fn share_a_group(first: &SessionSummary, second: &SessionSummary) -> bool {
        RepoGroup::of(first) == RepoGroup::of(second)
    }

    fn flat_lines(rows: &'a [SessionSummary]) -> Vec<TreeLine<'a>> {
        let flat = |row| TreeLine::session(row, TreeBranch::Flat);
        rows.iter().map(flat).collect()
    }

    fn grouped_lines(groups: Vec<&'a [SessionSummary]>) -> Vec<TreeLine<'a>> {
        let numbered = groups.into_iter().zip(1..);
        numbered
            .flat_map(|(rows, number)| Self::group_lines(number, rows))
            .collect()
    }

    fn group_lines(number: usize, rows: &'a [SessionSummary]) -> Vec<TreeLine<'a>> {
        let heading = TreeLine::Heading(RepoHeading::new(number, rows));
        once(heading).chain(Self::branch_lines(rows)).collect()
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

    fn is_heading(&self) -> bool {
        matches!(self, Self::Heading(_))
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
            let session = ServiceFixture::session_at(name, repo, ServiceFixture::CREATED_AT);
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

        let tree = SessionTree::new(&rows);

        assert_eq!(
            TreeFixture::shapes(&tree),
            ["Flat dark-mode", "Flat fix-auth"]
        );
        assert!(!tree.is_grouped());
    }

    #[test]
    fn two_repos_give_a_heading_then_middle_and_last_branches() {
        let rows = TreeFixture::two_repos();

        let tree = SessionTree::new(&rows);

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
    fn the_selection_stays_on_screen_when_headings_use_up_lines() {
        let rows = TreeFixture::two_repos();
        let tree = SessionTree::new(&rows);

        let visible: Vec<String> = tree
            .visible(Some("docs"), 2)
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

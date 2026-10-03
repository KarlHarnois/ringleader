pub struct TableColumns {
    inner_width: usize,
    name: usize,
    branch: Option<usize>,
    diff: Option<usize>,
}

impl TableColumns {
    pub const MINIMUM_NAME_WIDTH: usize = 12;
    const INDICATOR_WIDTH: usize = 1;
    const GAP: usize = 1;
    const ELLIPSIS: char = '…';
    const NAME_TITLE: &'static str = "Name";
    const BRANCH_TITLE: &'static str = "Branch";
    const DIFF_TITLE: &'static str = "Diff";

    pub fn new(inner_width: usize, names: impl Iterator<Item = String>) -> Self {
        Self {
            inner_width,
            name: Self::widest(names, Self::NAME_TITLE).max(Self::MINIMUM_NAME_WIDTH),
            branch: None,
            diff: None,
        }
    }

    pub fn with_branch(self, branches: impl Iterator<Item = String>) -> Self {
        let branch = Some(Self::widest(branches, Self::BRANCH_TITLE));
        Self { branch, ..self }
    }

    pub fn with_diff(self, diffs: impl Iterator<Item = String>) -> Self {
        let diff = Some(Self::widest(diffs, Self::DIFF_TITLE));
        Self { diff, ..self }
    }

    pub fn fitted(mut self) -> Self {
        if self.width() > self.inner_width {
            self.branch = None;
        }
        if self.width() > self.inner_width {
            self.diff = None;
        }
        let room_for_name = self.inner_width.saturating_sub(self.width() - self.name);
        self.name = self.name.min(room_for_name).max(Self::MINIMUM_NAME_WIDTH);
        self
    }

    pub fn header(&self) -> String {
        format!(
            " {}",
            self.row(Self::NAME_TITLE, Self::BRANCH_TITLE, Self::DIFF_TITLE)
        )
    }

    pub fn row(&self, name: &str, branch: &str, diff: &str) -> String {
        let mut text = format!(" {}", Self::fit(name, self.name));
        if let Some(width) = self.branch {
            text.push_str(&format!(" {}", Self::fit(branch, width)));
        }
        if let Some(width) = self.diff {
            text.push_str(&format!(" {}", Self::fit(diff, width)));
        }
        let width = self.inner_width.saturating_sub(Self::INDICATOR_WIDTH);
        format!("{text:<width$}")
    }

    fn width(&self) -> usize {
        let optional = |column: Option<usize>| column.map_or(0, |width| width + Self::GAP);
        Self::INDICATOR_WIDTH + Self::GAP + self.name + optional(self.branch) + optional(self.diff)
    }

    fn widest(texts: impl Iterator<Item = String>, title: &str) -> usize {
        texts
            .map(|text| text.chars().count())
            .max()
            .unwrap_or(0)
            .max(title.len())
    }

    fn fit(text: &str, width: usize) -> String {
        if text.chars().count() <= width {
            return format!("{text:<width$}");
        }
        let kept: String = text.chars().take(width.saturating_sub(1)).collect();
        format!("{kept}{}", Self::ELLIPSIS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ColumnsFixture;

    impl ColumnsFixture {
        const NAME: &'static str = "fix-auth";
        const BRANCH: &'static str = "ringleader/fix-auth";
        const DIFF: &'static str = "+124 -31 ?2";

        fn fitted(inner_width: usize, name: &str) -> TableColumns {
            TableColumns::new(inner_width, [name.to_string()].into_iter())
                .with_branch([Self::BRANCH.to_string()].into_iter())
                .with_diff([Self::DIFF.to_string()].into_iter())
                .fitted()
        }

        fn row(inner_width: usize, name: &str) -> String {
            Self::fitted(inner_width, name).row(name, Self::BRANCH, Self::DIFF)
        }
    }

    #[test]
    fn a_wide_table_shows_name_branch_and_diff() {
        let row = ColumnsFixture::row(46, ColumnsFixture::NAME);

        assert_eq!(row, " fix-auth     ringleader/fix-auth +124 -31 ?2");
    }

    #[test]
    fn a_tight_table_drops_the_branch_first() {
        let row = ColumnsFixture::row(40, ColumnsFixture::NAME);

        assert_eq!(row.trim_end(), " fix-auth     +124 -31 ?2");
    }

    #[test]
    fn a_tighter_table_drops_the_diff_next() {
        let row = ColumnsFixture::row(20, ColumnsFixture::NAME);

        assert_eq!(row.trim_end(), " fix-auth");
    }

    #[test]
    fn a_long_name_truncates_with_an_ellipsis_down_to_twelve_columns() {
        let row = ColumnsFixture::row(10, "refactor-the-session-reconciler");

        assert_eq!(row, " refactor-th…");
    }

    #[test]
    fn a_long_name_uses_the_room_left_after_dropping_columns() {
        let row = ColumnsFixture::row(20, "refactor-the-session-reconciler");

        assert_eq!(row, " refactor-the-sess…");
    }

    #[test]
    fn the_header_names_each_shown_column() {
        let columns = ColumnsFixture::fitted(48, ColumnsFixture::NAME);

        assert_eq!(
            columns.header().trim_end(),
            "  Name         Branch              Diff"
        );
    }
}

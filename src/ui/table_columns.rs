use super::text_fit::TextFit;

pub struct TableColumns {
    inner_width: usize,
    name: usize,
    agent: Option<usize>,
    pull_request: Option<usize>,
    diff: Option<usize>,
}

impl TableColumns {
    pub const MINIMUM_NAME_WIDTH: usize = 12;
    const INDICATOR_WIDTH: usize = 1;
    const INDICATOR_GAP: &'static str = " ";
    const COLUMN_GAP: &'static str = "  ";
    const NAME_TITLE: &'static str = "Name";
    const AGENT_TITLE: &'static str = "Agent";
    const PULL_REQUEST_TITLE: &'static str = "PR";
    const DIFF_TITLE: &'static str = "Diff";

    pub fn new(inner_width: usize, names: impl Iterator<Item = String>) -> Self {
        Self {
            inner_width,
            name: Self::widest(names, Self::NAME_TITLE).max(Self::MINIMUM_NAME_WIDTH),
            agent: None,
            pull_request: None,
            diff: None,
        }
    }

    pub fn with_agent(self, agents: impl Iterator<Item = String>) -> Self {
        let agent = Some(Self::widest(agents, Self::AGENT_TITLE));
        Self { agent, ..self }
    }

    pub fn with_pull_request(self, pull_requests: impl Iterator<Item = String>) -> Self {
        let pull_request = Some(Self::widest(pull_requests, Self::PULL_REQUEST_TITLE));
        Self {
            pull_request,
            ..self
        }
    }

    pub fn with_diff(self, diffs: impl Iterator<Item = String>) -> Self {
        let diff = Some(Self::widest(diffs, Self::DIFF_TITLE));
        Self { diff, ..self }
    }

    pub fn fitted(mut self) -> Self {
        if self.overflows() {
            self.agent = None;
        }
        if self.overflows() {
            self.pull_request = None;
        }
        if self.overflows() {
            self.diff = None;
        }
        let room_for_name = self.inner_width.saturating_sub(self.width() - self.name);
        self.name = self.name.min(room_for_name).max(Self::MINIMUM_NAME_WIDTH);
        self
    }

    pub fn header(&self) -> String {
        let titles = RowCells::new(Self::NAME_TITLE)
            .with_agent(Self::AGENT_TITLE)
            .with_pull_request(Self::PULL_REQUEST_TITLE)
            .with_diff(Self::DIFF_TITLE);
        format!(" {}", self.row(&titles))
    }

    pub fn row(&self, cells: &RowCells) -> String {
        let mut text = format!("{}{}", Self::INDICATOR_GAP, self.name_cell(&cells.name));
        for (width, cell) in self.optional_cells(cells) {
            text.push_str(Self::COLUMN_GAP);
            text.push_str(&Self::fit_text(cell, width));
        }
        let width = self.inner_width.saturating_sub(Self::INDICATOR_WIDTH);
        format!("{text:<width$}")
    }

    pub fn name_cell(&self, name: &str) -> String {
        Self::fit_text(name, self.name)
    }

    fn optional_cells<'c>(&self, cells: &'c RowCells) -> impl Iterator<Item = (usize, &'c str)> {
        let columns = [
            (self.agent, &cells.agent),
            (self.pull_request, &cells.pull_request),
            (self.diff, &cells.diff),
        ];
        columns
            .into_iter()
            .filter_map(|(width, cell)| width.map(|width| (width, cell.as_str())))
    }

    fn overflows(&self) -> bool {
        self.width() > self.inner_width
    }

    fn width(&self) -> usize {
        let optional = [self.agent, self.pull_request, self.diff]
            .into_iter()
            .flatten();
        let column_gap = Self::COLUMN_GAP.chars().count();
        let optional_width: usize = optional.map(|width| width + column_gap).sum();
        Self::INDICATOR_WIDTH + Self::INDICATOR_GAP.chars().count() + self.name + optional_width
    }

    fn fit_text(text: &str, width: usize) -> String {
        TextFit::new(text).padded(width)
    }

    fn widest(texts: impl Iterator<Item = String>, title: &str) -> usize {
        texts
            .map(|text| text.chars().count())
            .max()
            .unwrap_or(0)
            .max(title.len())
    }
}

pub struct RowCells {
    name: String,
    agent: String,
    pull_request: String,
    diff: String,
}

impl RowCells {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            agent: String::new(),
            pull_request: String::new(),
            diff: String::new(),
        }
    }

    pub fn with_agent(self, agent: impl Into<String>) -> Self {
        let agent = agent.into();
        Self { agent, ..self }
    }

    pub fn with_pull_request(self, pull_request: impl Into<String>) -> Self {
        let pull_request = pull_request.into();
        Self {
            pull_request,
            ..self
        }
    }

    pub fn with_diff(self, diff: impl Into<String>) -> Self {
        let diff = diff.into();
        Self { diff, ..self }
    }

    pub fn agent(&self) -> &str {
        &self.agent
    }

    pub fn pull_request(&self) -> &str {
        &self.pull_request
    }

    pub fn diff(&self) -> &str {
        &self.diff
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ColumnsFixture;

    impl ColumnsFixture {
        const NAME: &'static str = "fix-auth";
        const PULL_REQUEST: &'static str = "#142";
        const DIFF: &'static str = "+124 -31 ?2";

        const AGENT: &'static str = "claude";

        fn fitted(inner_width: usize, name: &str) -> TableColumns {
            TableColumns::new(inner_width, [name.to_string()].into_iter())
                .with_pull_request([Self::PULL_REQUEST.to_string()].into_iter())
                .with_diff([Self::DIFF.to_string()].into_iter())
                .fitted()
        }

        fn fitted_with_agent(inner_width: usize) -> TableColumns {
            TableColumns::new(inner_width, [Self::NAME.to_string()].into_iter())
                .with_agent([Self::AGENT.to_string()].into_iter())
                .with_pull_request([Self::PULL_REQUEST.to_string()].into_iter())
                .with_diff([Self::DIFF.to_string()].into_iter())
                .fitted()
        }

        fn cells(name: &str) -> RowCells {
            RowCells::new(name)
                .with_agent(Self::AGENT)
                .with_pull_request(Self::PULL_REQUEST)
                .with_diff(Self::DIFF)
        }

        fn row(inner_width: usize, name: &str) -> String {
            Self::fitted(inner_width, name).row(&Self::cells(name))
        }

        fn row_with_agent(inner_width: usize) -> String {
            Self::fitted_with_agent(inner_width).row(&Self::cells(Self::NAME))
        }
    }

    #[test]
    fn a_wide_table_shows_name_pull_request_and_diff() {
        let row = ColumnsFixture::row(33, ColumnsFixture::NAME);

        assert_eq!(row, " fix-auth      #142  +124 -31 ?2");
    }

    #[test]
    fn a_tight_table_drops_the_pull_request_first() {
        let row = ColumnsFixture::row(32, ColumnsFixture::NAME);

        assert_eq!(row.trim_end(), " fix-auth      +124 -31 ?2");
    }

    #[test]
    fn the_tightest_table_drops_the_diff_last() {
        let row = ColumnsFixture::row(26, ColumnsFixture::NAME);

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
    fn the_agent_follows_the_name() {
        let row = ColumnsFixture::row_with_agent(41);

        assert_eq!(row, " fix-auth      claude  #142  +124 -31 ?2");
    }

    #[test]
    fn a_tight_table_drops_the_agent_before_the_pull_request() {
        let row = ColumnsFixture::row_with_agent(40);

        assert_eq!(row.trim_end(), " fix-auth      #142  +124 -31 ?2");
    }

    #[test]
    fn a_header_with_an_agent_column_names_it() {
        let columns = ColumnsFixture::fitted_with_agent(41);

        assert_eq!(
            columns.header().trim_end(),
            "  Name          Agent   PR    Diff"
        );
    }

    #[test]
    fn the_header_names_each_shown_column() {
        let columns = ColumnsFixture::fitted(32, ColumnsFixture::NAME);

        assert_eq!(columns.header().trim_end(), "  Name          Diff");
    }

    #[test]
    fn a_wide_header_names_the_pull_request_column() {
        let columns = ColumnsFixture::fitted(33, ColumnsFixture::NAME);

        assert_eq!(columns.header().trim_end(), "  Name          PR    Diff");
    }
}

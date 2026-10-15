#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeBases {
    bases: Vec<String>,
}

impl MergeBases {
    pub fn new(bases: Vec<String>) -> Self {
        Self { bases }
    }

    pub fn measured_from<'a>(&'a self, recorded: &'a str) -> &'a str {
        let through_the_base_branch = self.bases.iter().find(|base| *base != recorded);
        through_the_base_branch
            .or_else(|| self.bases.first())
            .map_or(recorded, String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BasesFixture;

    impl BasesFixture {
        const RECORDED: &'static str = "recorded";
        const UPSTREAM: &'static str = "upstream";

        fn of(bases: &[&str]) -> MergeBases {
            MergeBases::new(bases.iter().map(ToString::to_string).collect())
        }
    }

    #[test]
    fn a_tie_measures_from_the_base_that_is_not_the_recorded_commit() {
        let bases = BasesFixture::of(&[BasesFixture::RECORDED, BasesFixture::UPSTREAM]);

        assert_eq!(
            bases.measured_from(BasesFixture::RECORDED),
            BasesFixture::UPSTREAM
        );
    }

    #[test]
    fn the_recorded_commit_alone_is_measured_from() {
        let bases = BasesFixture::of(&[BasesFixture::RECORDED]);

        assert_eq!(
            bases.measured_from(BasesFixture::RECORDED),
            BasesFixture::RECORDED
        );
    }

    #[test]
    fn no_base_falls_back_to_the_recorded_commit() {
        let bases = BasesFixture::of(&[]);

        assert_eq!(
            bases.measured_from(BasesFixture::RECORDED),
            BasesFixture::RECORDED
        );
    }
}

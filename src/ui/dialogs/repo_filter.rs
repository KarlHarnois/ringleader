use super::fuzzy_ranker::FuzzyRanker;
use super::home_path::HomePath;
use crate::domain::Repo;

pub struct RepoFilter {
    ranker: FuzzyRanker,
    home: HomePath,
    shown_paths: Vec<String>,
}

impl RepoFilter {
    pub fn new(home: HomePath) -> Self {
        Self {
            ranker: FuzzyRanker::for_paths(),
            home,
            shown_paths: Vec::new(),
        }
    }

    pub fn load(&mut self, repos: &[Repo]) {
        self.shown_paths = repos
            .iter()
            .map(|repo| self.home.shown(repo.path()))
            .collect();
    }

    pub fn shown_path(&self, index: usize) -> &str {
        &self.shown_paths[index]
    }

    pub fn ranked(&mut self, query: &str) -> Vec<usize> {
        self.ranker.ranked(query, &self.shown_paths)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::domain::RepoName;

    struct FilterFixture;

    impl FilterFixture {
        const HOME: &'static str = "/home/example";

        fn filter(paths: &[&str]) -> RepoFilter {
            let repos: Vec<Repo> = paths.iter().map(|path| Self::repo(path)).collect();
            let mut filter = RepoFilter::new(HomePath::new(Some(Path::new(Self::HOME))));
            filter.load(&repos);
            filter
        }

        fn repo(path: &str) -> Repo {
            let path = Path::new(path);
            Repo::discovered(RepoName::of_directory(path).unwrap(), path)
        }

        fn ranked_paths(filter: &mut RepoFilter, query: &str) -> Vec<String> {
            let ranked = filter.ranked(query);
            ranked
                .into_iter()
                .map(|index| filter.shown_path(index).to_string())
                .collect()
        }
    }

    #[test]
    fn an_empty_query_keeps_every_repo_in_order() {
        let mut filter = FilterFixture::filter(&["/home/example/zeta", "/home/example/alpha"]);

        assert_eq!(
            FilterFixture::ranked_paths(&mut filter, ""),
            ["~/zeta", "~/alpha"]
        );
    }

    #[test]
    fn a_query_keeps_only_paths_holding_its_letters_in_order() {
        let mut filter = FilterFixture::filter(&[
            "/home/example/hobby/ringleader",
            "/home/example/work/api-gateway",
            "/home/example/notes/reading-list",
        ]);

        assert_eq!(
            FilterFixture::ranked_paths(&mut filter, "rld"),
            ["~/hobby/ringleader"]
        );
    }

    #[test]
    fn a_match_at_the_start_of_a_folder_name_ranks_first() {
        let mut filter = FilterFixture::filter(&[
            "/home/example/work/bring-up",
            "/home/example/hobby/ringleader",
        ]);

        assert_eq!(
            FilterFixture::ranked_paths(&mut filter, "ring"),
            ["~/hobby/ringleader", "~/work/bring-up"]
        );
    }

    #[test]
    fn an_uppercase_query_matches_case_exactly() {
        let mut filter =
            FilterFixture::filter(&["/home/example/ringleader", "/home/example/Ringtone"]);

        assert_eq!(
            FilterFixture::ranked_paths(&mut filter, "Ring"),
            ["~/Ringtone"]
        );
        assert_eq!(FilterFixture::ranked_paths(&mut filter, "ring").len(), 2);
    }
}

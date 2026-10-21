use std::cmp::Reverse;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

pub struct FuzzyRanker {
    matcher: Matcher,
}

impl FuzzyRanker {
    pub fn for_paths() -> Self {
        let mut config = Config::DEFAULT;
        config.set_match_paths();
        Self {
            matcher: Matcher::new(config),
        }
    }

    pub fn for_names() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT),
        }
    }

    pub fn ranked(&mut self, query: &str, candidates: &[String]) -> Vec<usize> {
        let pattern = Self::pattern(query);
        let mut buffer = Vec::new();
        let scored = candidates
            .iter()
            .enumerate()
            .filter_map(|(index, candidate)| {
                let haystack = Utf32Str::new(candidate, &mut buffer);
                let score = pattern.score(haystack, &mut self.matcher)?;
                Some((score, index))
            });
        Self::best_first(scored.collect())
    }

    pub fn matches(&mut self, query: &str, candidates: &[String]) -> Vec<FuzzyMatch> {
        let pattern = Self::pattern(query);
        let mut buffer = Vec::new();
        let scored = candidates
            .iter()
            .enumerate()
            .filter_map(|(index, candidate)| {
                let haystack = Utf32Str::new(candidate, &mut buffer);
                let mut positions = Vec::new();
                let score = pattern.indices(haystack, &mut self.matcher, &mut positions)?;
                let positions = Self::sorted_unique(positions);
                Some((score, FuzzyMatch { index, positions }))
            });
        Self::best_first(scored.collect())
    }

    fn best_first<T>(mut scored: Vec<(u32, T)>) -> Vec<T> {
        scored.sort_by_key(|(score, _)| Reverse(*score));
        scored.into_iter().map(|(_, found)| found).collect()
    }

    fn pattern(query: &str) -> Pattern {
        Pattern::parse(query, CaseMatching::Smart, Normalization::Smart)
    }

    fn sorted_unique(mut positions: Vec<u32>) -> Vec<usize> {
        positions.sort_unstable();
        positions.dedup();
        positions
            .into_iter()
            .filter_map(|position| usize::try_from(position).ok())
            .collect()
    }
}

pub struct FuzzyMatch {
    index: usize,
    positions: Vec<usize>,
}

impl FuzzyMatch {
    pub fn index(&self) -> usize {
        self.index
    }

    pub fn positions(&self) -> &[usize] {
        &self.positions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RankerFixture;

    impl RankerFixture {
        fn ranked_names(query: &str, names: &[&str]) -> Vec<String> {
            let candidates: Vec<String> = names.iter().map(|name| name.to_string()).collect();
            let ranked = FuzzyRanker::for_names().ranked(query, &candidates);
            ranked
                .into_iter()
                .map(|index| candidates[index].clone())
                .collect()
        }

        fn matched_positions(query: &str, candidate: &str) -> Option<Vec<usize>> {
            let found = FuzzyRanker::for_names().matches(query, &[candidate.to_string()]);
            found
                .first()
                .map(|fuzzy_match| fuzzy_match.positions().to_vec())
        }
    }

    #[test]
    fn an_empty_query_keeps_every_name_in_order() {
        assert_eq!(
            RankerFixture::ranked_names("", &["zeta", "alpha"]),
            ["zeta", "alpha"]
        );
    }

    #[test]
    fn a_name_query_keeps_only_names_holding_its_letters_in_order() {
        assert_eq!(
            RankerFixture::ranked_names("fa", &["dark-mode", "fix-auth", "docs"]),
            ["fix-auth"]
        );
    }

    #[test]
    fn a_match_at_the_start_of_a_word_ranks_first() {
        assert_eq!(
            RankerFixture::ranked_names("auth", &["unauthorized-retry", "fix-auth"]),
            ["fix-auth", "unauthorized-retry"]
        );
    }

    #[test]
    fn a_match_names_the_positions_of_the_letters_it_picked() {
        assert_eq!(
            RankerFixture::matched_positions("fa", "fix-auth"),
            Some(vec![0, 4])
        );
    }

    #[test]
    fn positions_from_several_words_come_sorted_without_repeats() {
        assert_eq!(
            RankerFixture::matched_positions("auth fix f", "fix-auth"),
            Some(vec![0, 1, 2, 4, 5, 6, 7])
        );
    }

    #[test]
    fn a_miss_on_a_later_word_leaves_no_positions() {
        assert_eq!(
            RankerFixture::matched_positions("fix zzz", "fix-auth"),
            None
        );
    }
}

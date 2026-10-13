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
        let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
        let mut buffer = Vec::new();
        let mut scored: Vec<(usize, u32)> = candidates
            .iter()
            .enumerate()
            .filter_map(|(index, candidate)| {
                let haystack = Utf32Str::new(candidate, &mut buffer);
                let score = pattern.score(haystack, &mut self.matcher)?;
                Some((index, score))
            })
            .collect();
        scored.sort_by_key(|(_, score)| Reverse(*score));
        scored.into_iter().map(|(index, _)| index).collect()
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
}

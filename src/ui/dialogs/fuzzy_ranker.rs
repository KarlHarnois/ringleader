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

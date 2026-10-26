use std::collections::HashSet;

use crate::domain::RepoGroup;

pub struct FoldChange {
    folded: HashSet<RepoGroup>,
    unfolded: HashSet<RepoGroup>,
}

impl FoldChange {
    pub fn between(before: &HashSet<RepoGroup>, after: &HashSet<RepoGroup>) -> Self {
        Self {
            folded: after.difference(before).cloned().collect(),
            unfolded: before.difference(after).cloned().collect(),
        }
    }

    pub fn applied_to(&self, stored: HashSet<RepoGroup>) -> HashSet<RepoGroup> {
        stored
            .into_iter()
            .filter(|group| !self.unfolded.contains(group))
            .chain(self.folded.iter().cloned())
            .collect()
    }
}

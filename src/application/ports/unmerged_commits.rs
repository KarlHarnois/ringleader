#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UnmergedCommits {
    count: u64,
    tip: Option<String>,
    head: Option<String>,
}

impl UnmergedCommits {
    const NO_COMMIT: &'static str = "none";

    pub fn new(count: u64, tip: Option<&str>, head: Option<&str>) -> Self {
        Self {
            count,
            tip: tip.map(str::to_string),
            head: head.map(str::to_string),
        }
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn tip(&self) -> Option<&str> {
        self.tip.as_deref()
    }

    pub fn head(&self) -> Option<&str> {
        self.head.as_deref()
    }

    pub fn tip_label(&self) -> &str {
        self.tip().unwrap_or(Self::NO_COMMIT)
    }

    pub fn head_label(&self) -> &str {
        self.head().unwrap_or(Self::NO_COMMIT)
    }

    pub fn same_commits_as(&self, other: &UnmergedCommits) -> bool {
        self.tip == other.tip && self.head == other.head
    }
}

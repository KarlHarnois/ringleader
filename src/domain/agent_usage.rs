use super::spend::Spend;
use super::usage_limit::UsageLimit;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageRow {
    Limit(UsageLimit),
    Spend(Spend),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentUsage {
    rows: Vec<UsageRow>,
}

impl AgentUsage {
    pub fn new(rows: Vec<UsageRow>) -> Self {
        Self { rows }
    }

    pub fn rows(&self) -> &[UsageRow] {
        &self.rows
    }
}

use super::spend::Spend;
use super::token_totals::TokenTotals;
use super::usage_limit::UsageLimit;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageRow {
    Limit(UsageLimit),
    Spend(Spend),
    Tokens(TokenTotals),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentUsage {
    model: Option<String>,
    rows: Vec<UsageRow>,
}

impl AgentUsage {
    pub fn new(rows: Vec<UsageRow>) -> Self {
        Self { model: None, rows }
    }

    pub fn with_model(self, model: &str) -> Self {
        Self {
            model: Some(model.to_string()),
            ..self
        }
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    pub fn rows(&self) -> &[UsageRow] {
        &self.rows
    }
}

use super::money::Money;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpendKind {
    ExtraUsage,
    Credits,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spend {
    kind: SpendKind,
    used: Money,
    limit: Option<Money>,
}

impl Spend {
    pub fn new(kind: SpendKind, used: Money) -> Self {
        Self {
            kind,
            used,
            limit: None,
        }
    }

    pub fn with_limit(self, limit: Money) -> Self {
        Self {
            limit: Some(limit),
            ..self
        }
    }

    pub fn kind(&self) -> SpendKind {
        self.kind
    }

    pub fn used(&self) -> &Money {
        &self.used
    }

    pub fn limit(&self) -> Option<&Money> {
        self.limit.as_ref()
    }
}

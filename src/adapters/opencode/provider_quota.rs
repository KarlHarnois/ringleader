use thiserror::Error;

use crate::domain::UsageRow;

pub trait ProviderQuota: Send + Sync {
    fn measure(&self, provider: &str) -> Result<Option<Vec<UsageRow>>, QuotaError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum QuotaError {
    #[error("the provider refused the key")]
    Refused,
    #[error("the provider asked to wait before the next quota query")]
    Throttled,
    #[error("the provider did not report the quota")]
    Unanswered,
    #[error("the provider took too long to report the quota")]
    TimedOut,
    #[error("the provider's quota could not be read")]
    Unreadable,
}

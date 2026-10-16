use crate::adapters::opencode::provider_quota::{ProviderQuota, QuotaError};
use crate::domain::UsageRow;

pub struct NullProviderQuota;

impl ProviderQuota for NullProviderQuota {
    fn measure(&self, _provider: &str) -> Result<Option<Vec<UsageRow>>, QuotaError> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_provider_has_no_quota() {
        assert_eq!(NullProviderQuota.measure("ollama-cloud"), Ok(None));
    }
}

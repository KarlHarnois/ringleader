use crate::adapters::opencode::provider_quota::{ProviderQuota, QuotaError};
use crate::domain::{Agent, UsageRow};

pub struct NullProviderQuota;

impl ProviderQuota for NullProviderQuota {
    fn measure(
        &self,
        _agent: &Agent,
        _provider: &str,
    ) -> Result<Option<Vec<UsageRow>>, QuotaError> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_provider_has_no_quota() {
        let agent = Agent::new("opencode", "opencode");

        assert_eq!(NullProviderQuota.measure(&agent, "ollama-cloud"), Ok(None));
    }
}

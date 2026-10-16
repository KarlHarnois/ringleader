use serde::Deserialize;

use crate::adapters::opencode::provider_quota::QuotaError;
use crate::domain::{LimitWindow, Money, Spend, SpendKind, UsageLimit, UsageRow};

#[derive(Deserialize)]
pub struct OllamaBalance {
    included: IncludedCredits,
}

impl OllamaBalance {
    pub fn parse(body: &str) -> Result<Vec<UsageRow>, QuotaError> {
        let balance: Self = serde_json::from_str(body).map_err(|error| {
            log::info!("could not read the Ollama balance: {error}");
            QuotaError::Unreadable
        })?;
        Ok(balance.included.rows())
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum IncludedCredits {
    Allowance {
        balance_usd: f64,
        allowance_usd: f64,
        period: Period,
    },
    Windows {
        session: Window,
        weekly: Window,
    },
}

impl IncludedCredits {
    const CURRENCY: &'static str = "USD";
    const FULL_PERCENT: f64 = 100.0;

    fn rows(self) -> Vec<UsageRow> {
        match self {
            Self::Allowance {
                balance_usd,
                allowance_usd,
                period,
            } => Self::allowance_rows(balance_usd, allowance_usd, &period),
            Self::Windows { session, weekly } => vec![
                session.usage_row(LimitWindow::Session),
                weekly.usage_row(LimitWindow::Week),
            ],
        }
    }

    fn allowance_rows(balance: f64, allowance: f64, period: &Period) -> Vec<UsageRow> {
        let used = (allowance - balance).max(0.0);
        let percent = used / allowance * Self::FULL_PERCENT;
        let spend = Spend::new(SpendKind::Credits, Money::major_units(used, Self::CURRENCY))
            .with_limit(Money::major_units(allowance, Self::CURRENCY));
        vec![
            UsageRow::Limit(UsageLimit::used(
                LimitWindow::Credits,
                percent,
                period.until.epoch_seconds(),
            )),
            UsageRow::Spend(spend),
        ]
    }
}

#[derive(Deserialize)]
struct Period {
    until: ResetTime,
}

#[derive(Deserialize)]
struct Window {
    remaining_percent: f64,
    resets_at: ResetTime,
}

impl Window {
    fn usage_row(&self, window: LimitWindow) -> UsageRow {
        UsageRow::Limit(UsageLimit::remaining(
            window,
            self.remaining_percent,
            self.resets_at.epoch_seconds(),
        ))
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
struct ResetTime(String);

impl ResetTime {
    fn epoch_seconds(&self) -> Option<u64> {
        let reset: jiff::Timestamp = self.0.parse().ok()?;
        u64::try_from(reset.as_second()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BalanceFixture;

    impl BalanceFixture {
        const CREDITS: &'static str =
            include_str!("../../../tests/fixtures/ollama-balance-credits.json");
        const LEGACY: &'static str =
            include_str!("../../../tests/fixtures/ollama-balance-legacy.json");
        const PERIOD_END: u64 = 1_792_056_600;
        const SESSION_RESET: u64 = 1_790_838_000;
        const WEEK_RESET: u64 = 1_791_158_400;

        fn allowance(balance: f64, allowance: f64) -> Vec<UsageRow> {
            let body = format!(
                r#"{{"included":{{"balance_usd":{balance},"allowance_usd":{allowance},"period":{{"from":"2026-09-15T09:30:00Z","until":"2026-10-15T09:30:00Z"}}}},"purchased":{{"balance_usd":0}}}}"#
            );
            OllamaBalance::parse(&body).unwrap()
        }

        fn credits_limit(rows: &[UsageRow]) -> &UsageLimit {
            match &rows[0] {
                UsageRow::Limit(limit) => limit,
                row => panic!("expected a limit, got {row:?}"),
            }
        }
    }

    #[test]
    fn a_credit_plan_shows_the_share_of_its_allowance_used_until_the_period_ends() {
        let rows = OllamaBalance::parse(BalanceFixture::CREDITS).unwrap();

        assert_eq!(
            rows[0],
            UsageRow::Limit(UsageLimit::used(
                LimitWindow::Credits,
                27.5,
                Some(BalanceFixture::PERIOD_END)
            ))
        );
    }

    #[test]
    fn a_credit_plan_spends_its_allowance_less_the_balance_left() {
        let rows = OllamaBalance::parse(BalanceFixture::CREDITS).unwrap();

        assert_eq!(
            rows[1],
            UsageRow::Spend(
                Spend::new(SpendKind::Credits, Money::minor_units(2750, "USD"))
                    .with_limit(Money::minor_units(10_000, "USD"))
            )
        );
    }

    #[test]
    fn a_plan_without_an_allowance_has_used_none_of_it() {
        let rows = BalanceFixture::allowance(0.0, 0.0);

        assert_eq!(BalanceFixture::credits_limit(&rows).used_percent(), 0);
    }

    #[test]
    fn a_balance_above_the_allowance_has_used_none_of_it() {
        let rows = BalanceFixture::allowance(120.0, 100.0);

        assert_eq!(BalanceFixture::credits_limit(&rows).used_percent(), 0);
    }

    #[test]
    fn a_legacy_plan_shows_its_session_and_weekly_windows() {
        let rows = OllamaBalance::parse(BalanceFixture::LEGACY).unwrap();

        assert_eq!(
            rows,
            [
                UsageRow::Limit(UsageLimit::used(
                    LimitWindow::Session,
                    25.0,
                    Some(BalanceFixture::SESSION_RESET)
                )),
                UsageRow::Limit(UsageLimit::used(
                    LimitWindow::Week,
                    60.0,
                    Some(BalanceFixture::WEEK_RESET)
                )),
            ]
        );
    }

    #[test]
    fn a_response_of_neither_shape_is_unreadable() {
        assert_eq!(
            OllamaBalance::parse(r#"{"included":{},"purchased":{}}"#).unwrap_err(),
            QuotaError::Unreadable
        );
    }
}

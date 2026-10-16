use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::application::ports::UsageError;
use crate::domain::{AgentUsage, LimitWindow, Money, Spend, SpendKind, UsageLimit, UsageRow};

pub struct ClaudeUsageReport;

impl ClaudeUsageReport {
    const USAGE_MESSAGE: &'static str = "assistant";

    pub fn parse(stream: &str) -> Result<AgentUsage, UsageError> {
        let message = stream
            .lines()
            .filter_map(|line| serde_json::from_str::<StreamMessage>(line).ok())
            .find(|message| message.kind == Self::USAGE_MESSAGE)
            .ok_or(UsageError::Unreadable)?;
        let report: Option<UsageReport> = match message.usage_report {
            Some(report) => Some(serde_json::from_value(report).map_err(Self::unreadable)?),
            None => None,
        };
        let rate_limits = report.and_then(|report| report.rate_limits);
        Ok(AgentUsage::new(
            rate_limits.map(RateLimits::rows).unwrap_or_default(),
        ))
    }

    fn unreadable(error: serde_json::Error) -> UsageError {
        log::info!("could not read Claude's usage report: {error}");
        UsageError::Unreadable
    }

    fn readable_part<T: DeserializeOwned>(part: Value) -> Option<T> {
        serde_json::from_value(part)
            .map_err(|error| log::info!("skipped a part of Claude's usage report: {error}"))
            .ok()
    }
}

#[derive(Deserialize)]
struct StreamMessage {
    #[serde(rename = "type")]
    kind: String,
    usage_report: Option<Value>,
}

#[derive(Deserialize)]
struct UsageReport {
    rate_limits: Option<RateLimits>,
}

#[derive(Deserialize)]
struct RateLimits {
    limits: Option<Vec<Value>>,
    extra_usage: Option<Value>,
}

impl RateLimits {
    fn rows(self) -> Vec<UsageRow> {
        let limits = self
            .limits
            .unwrap_or_default()
            .into_iter()
            .filter_map(ClaudeUsageReport::readable_part::<RateLimit>);
        let spend = self
            .extra_usage
            .and_then(ClaudeUsageReport::readable_part::<ExtraUsage>)
            .and_then(ExtraUsage::spend);
        limits
            .map(|limit| UsageRow::Limit(limit.usage_limit()))
            .chain(spend.map(UsageRow::Spend))
            .collect()
    }
}

#[derive(Deserialize)]
struct RateLimit {
    kind: String,
    percent: f64,
    resets_at: Option<String>,
    scope: Option<LimitScope>,
}

impl RateLimit {
    const SESSION: &'static str = "session";
    const WEEK: &'static str = "weekly_all";

    fn usage_limit(&self) -> UsageLimit {
        UsageLimit::used(self.window(), self.percent, self.reset_epoch_seconds())
    }

    fn window(&self) -> LimitWindow {
        match self.kind.as_str() {
            Self::SESSION => LimitWindow::Session,
            Self::WEEK => LimitWindow::Week,
            _ => LimitWindow::Scoped(self.scope_name()),
        }
    }

    fn scope_name(&self) -> String {
        self.scope
            .as_ref()
            .and_then(LimitScope::name)
            .unwrap_or(&self.kind)
            .to_string()
    }

    fn reset_epoch_seconds(&self) -> Option<u64> {
        let timestamp: jiff::Timestamp = self.resets_at.as_deref()?.parse().ok()?;
        u64::try_from(timestamp.as_second()).ok()
    }
}

#[derive(Deserialize)]
struct LimitScope {
    model: Option<ScopeName>,
    surface: Option<ScopeName>,
}

impl LimitScope {
    fn name(&self) -> Option<&str> {
        let named = self.model.as_ref().or(self.surface.as_ref())?;
        Some(&named.display_name)
    }
}

#[derive(Deserialize)]
struct ScopeName {
    display_name: String,
}

#[derive(Deserialize)]
struct ExtraUsage {
    is_enabled: bool,
    monthly_limit: Option<f64>,
    used_credits: Option<f64>,
    currency: Option<String>,
}

impl ExtraUsage {
    const DEFAULT_CURRENCY: &'static str = "USD";

    fn spend(self) -> Option<Spend> {
        if !self.is_enabled {
            return None;
        }
        let spend = Spend::new(SpendKind::ExtraUsage, self.money(self.used_credits?));
        Some(match self.monthly_limit {
            Some(limit) => spend.with_limit(self.money(limit)),
            None => spend,
        })
    }

    fn money(&self, minor_units: f64) -> Money {
        let currency = self.currency.as_deref().unwrap_or(Self::DEFAULT_CURRENCY);
        Money::minor_units(minor_units.round() as u64, currency)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ReportFixture;

    impl ReportFixture {
        const RECORDED: &'static str =
            include_str!("../../../tests/fixtures/claude-usage-stream.jsonl");
        const API_KEY: &'static str =
            include_str!("../../../tests/fixtures/claude-usage-stream-api-key.jsonl");
        const SESSION_RESET: u64 = 1_767_233_640;
        const WEEK_RESET: u64 = 1_767_430_800;

        fn recorded() -> AgentUsage {
            ClaudeUsageReport::parse(Self::RECORDED).unwrap()
        }

        fn with_rate_limits(rate_limits: &str) -> AgentUsage {
            let stream = format!(
                "{{\"type\":\"assistant\",\"usage_report\":{{\"rate_limits\":{rate_limits}}}}}"
            );
            ClaudeUsageReport::parse(&stream).unwrap()
        }

        fn with_limit(limit: &str) -> AgentUsage {
            Self::with_rate_limits(&format!("{{\"limits\":[{limit}]}}"))
        }

        fn with_extra_usage(extra_usage: &str) -> AgentUsage {
            Self::with_rate_limits(&format!("{{\"limits\":[],\"extra_usage\":{extra_usage}}}"))
        }

        fn only_window(usage: &AgentUsage) -> LimitWindow {
            match usage.rows() {
                [UsageRow::Limit(limit)] => limit.window().clone(),
                rows => panic!("expected one limit, got {rows:?}"),
            }
        }

        fn spend(minor_units: u64, currency: &str) -> Spend {
            Spend::new(
                SpendKind::ExtraUsage,
                Money::minor_units(minor_units, currency),
            )
        }
    }

    #[test]
    fn limits_keep_the_order_the_server_sent() {
        let rows = ReportFixture::recorded().rows().to_vec();

        assert_eq!(
            rows[..3],
            [
                UsageRow::Limit(UsageLimit::used(
                    LimitWindow::Session,
                    42.0,
                    Some(ReportFixture::SESSION_RESET)
                )),
                UsageRow::Limit(UsageLimit::used(
                    LimitWindow::Week,
                    18.0,
                    Some(ReportFixture::WEEK_RESET)
                )),
                UsageRow::Limit(UsageLimit::used(
                    LimitWindow::Scoped("Example Model".to_string()),
                    5.0,
                    Some(ReportFixture::WEEK_RESET)
                )),
            ]
        );
    }

    #[test]
    fn enabled_extra_usage_is_spend_in_minor_units_against_its_limit() {
        let rows = ReportFixture::recorded().rows().to_vec();

        assert_eq!(
            rows[3],
            UsageRow::Spend(
                ReportFixture::spend(320, "USD").with_limit(Money::minor_units(5000, "USD"))
            )
        );
    }

    #[test]
    fn a_scope_without_a_model_is_named_after_its_surface() {
        let usage = ReportFixture::with_limit(
            r#"{"kind":"weekly_scoped","percent":3,"scope":{"model":null,"surface":{"display_name":"Example Surface"}}}"#,
        );

        assert_eq!(
            ReportFixture::only_window(&usage),
            LimitWindow::Scoped("Example Surface".to_string())
        );
    }

    #[test]
    fn an_unknown_kind_without_a_scope_is_named_after_its_kind() {
        let usage = ReportFixture::with_limit(r#"{"kind":"monthly_example","percent":3}"#);

        assert_eq!(
            ReportFixture::only_window(&usage),
            LimitWindow::Scoped("monthly_example".to_string())
        );
    }

    #[test]
    fn an_unreadable_reset_time_is_left_out() {
        let usage = ReportFixture::with_limit(
            r#"{"kind":"session","percent":3,"resets_at":"next tuesday"}"#,
        );

        assert_eq!(
            usage.rows(),
            [UsageRow::Limit(UsageLimit::used(
                LimitWindow::Session,
                3.0,
                None
            ))]
        );
    }

    #[test]
    fn a_limit_that_cannot_be_read_is_skipped_and_the_others_kept() {
        let usage = ReportFixture::with_rate_limits(
            r#"{"limits":[{"kind":"session","percent":null},{"kind":"weekly_all","percent":18}]}"#,
        );

        assert_eq!(
            usage.rows(),
            [UsageRow::Limit(UsageLimit::used(
                LimitWindow::Week,
                18.0,
                None
            ))]
        );
    }

    #[test]
    fn extra_usage_that_cannot_be_read_is_skipped() {
        let usage = ReportFixture::with_extra_usage(r#"{"is_enabled":"maybe","used_credits":120}"#);

        assert!(usage.rows().is_empty());
    }

    #[test]
    fn a_usage_report_of_another_shape_is_unreadable() {
        assert_eq!(
            ClaudeUsageReport::parse(
                r#"{"type":"assistant","usage_report":{"rate_limits":"soon"}}"#
            ),
            Err(UsageError::Unreadable)
        );
    }

    #[test]
    fn disabled_extra_usage_adds_no_row() {
        let usage = ReportFixture::with_extra_usage(
            r#"{"is_enabled":false,"monthly_limit":5000,"used_credits":0,"currency":"USD"}"#,
        );

        assert!(usage.rows().is_empty());
    }

    #[test]
    fn extra_usage_without_a_cap_has_no_limit() {
        let usage = ReportFixture::with_extra_usage(
            r#"{"is_enabled":true,"monthly_limit":null,"used_credits":120,"currency":"CAD"}"#,
        );

        assert_eq!(
            usage.rows(),
            [UsageRow::Spend(ReportFixture::spend(120, "CAD"))]
        );
    }

    #[test]
    fn extra_usage_without_a_currency_is_in_dollars() {
        let usage = ReportFixture::with_extra_usage(r#"{"is_enabled":true,"used_credits":120}"#);

        assert_eq!(
            usage.rows(),
            [UsageRow::Spend(ReportFixture::spend(120, "USD"))]
        );
    }

    #[test]
    fn extra_usage_without_a_used_amount_adds_no_row() {
        let usage = ReportFixture::with_extra_usage(r#"{"is_enabled":true,"used_credits":null}"#);

        assert!(usage.rows().is_empty());
    }

    #[test]
    fn an_api_key_session_reports_no_plan_usage() {
        let usage = ClaudeUsageReport::parse(ReportFixture::API_KEY).unwrap();

        assert!(usage.rows().is_empty());
    }

    #[test]
    fn null_rate_limits_report_no_plan_usage() {
        assert!(ReportFixture::with_rate_limits("null").rows().is_empty());
    }

    #[test]
    fn output_without_a_usage_message_is_unreadable() {
        assert_eq!(
            ClaudeUsageReport::parse("{\"type\":\"result\"}\nnot json\n"),
            Err(UsageError::Unreadable)
        );
    }
}

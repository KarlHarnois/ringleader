use serde::Deserialize;

use crate::application::ports::UsageError;
use crate::domain::{AgentUsage, Money, TokenTotals, TotalsWindow, UsageRow};

#[derive(Debug, Deserialize)]
pub struct OpencodeActivity {
    provider: Option<String>,
    model: Option<String>,
    tokens_today: f64,
    cost_today: f64,
    tokens_last_seven_days: f64,
    cost_last_seven_days: f64,
}

impl OpencodeActivity {
    pub const QUERY: &'static str = "\
        with active as ( \
          select json_extract(model, '$.providerID') as provider, \
            json_extract(model, '$.id') as model \
          from session where model is not null order by time_updated desc limit 1 \
        ), \
        recent as ( \
          select message.time_created as created, \
            json_extract(message.data, '$.cost') as cost, \
            coalesce(json_extract(message.data, '$.tokens.input'), 0) \
              + coalesce(json_extract(message.data, '$.tokens.output'), 0) \
              + coalesce(json_extract(message.data, '$.tokens.reasoning'), 0) as tokens \
          from message, active \
          where message.time_created \
              >= cast(strftime('%s', 'now', 'localtime', 'start of day', '-6 days', 'utc') as integer) \
                * 1000 \
            and json_extract(message.data, '$.role') = 'assistant' \
            and json_extract(message.data, '$.providerID') = active.provider \
        ), \
        today as ( \
          select * from recent \
          where created \
            >= cast(strftime('%s', 'now', 'localtime', 'start of day', 'utc') as integer) * 1000 \
        ) \
        select active.provider, active.model, \
          (select coalesce(sum(tokens), 0) from today) as tokens_today, \
          (select coalesce(sum(cost), 0) from today) as cost_today, \
          (select coalesce(sum(tokens), 0) from recent) as tokens_last_seven_days, \
          (select coalesce(sum(cost), 0) from recent) as cost_last_seven_days \
        from active";
    const LIST_PRICE_CURRENCY: &'static str = "USD";

    pub fn parse(output: &str) -> Result<Option<Self>, UsageError> {
        if output.trim().is_empty() {
            return Ok(None);
        }
        let rows: Vec<Self> = serde_json::from_str(output).map_err(|error| {
            log::info!("could not read opencode's activity: {error}");
            UsageError::Unreadable
        })?;
        Ok(rows.into_iter().next())
    }

    pub fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }

    pub fn usage(&self) -> AgentUsage {
        self.usage_with(vec![
            Self::totals(TotalsWindow::Today, self.tokens_today, self.cost_today),
            Self::totals(
                TotalsWindow::LastSevenDays,
                self.tokens_last_seven_days,
                self.cost_last_seven_days,
            ),
        ])
    }

    pub fn usage_with(&self, rows: Vec<UsageRow>) -> AgentUsage {
        match self.model_name() {
            Some(model) => AgentUsage::new(rows).with_model(&model),
            None => AgentUsage::new(rows),
        }
    }

    fn totals(window: TotalsWindow, tokens: f64, cost: f64) -> UsageRow {
        let list_price = Money::major_units(cost, Self::LIST_PRICE_CURRENCY);
        UsageRow::Tokens(TokenTotals::new(
            window,
            tokens.round().max(0.0) as u64,
            list_price,
        ))
    }

    fn model_name(&self) -> Option<String> {
        Some(format!(
            "{}/{}",
            self.provider.as_ref()?,
            self.model.as_ref()?
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ActivityFixture;

    impl ActivityFixture {
        const RECORDED: &'static str =
            include_str!("../../../tests/fixtures/opencode-db-activity.json");

        fn recorded() -> AgentUsage {
            OpencodeActivity::parse(Self::RECORDED)
                .unwrap()
                .unwrap()
                .usage()
        }

        fn totals(window: TotalsWindow, tokens: u64, cents: u64) -> UsageRow {
            UsageRow::Tokens(TokenTotals::new(
                window,
                tokens,
                Money::minor_units(cents, "USD"),
            ))
        }
    }

    #[test]
    fn the_usage_names_the_active_provider_and_model() {
        assert_eq!(
            ActivityFixture::recorded().model(),
            Some("example-provider/example-model")
        );
    }

    #[test]
    fn the_usage_totals_today_and_the_last_seven_days_at_list_price() {
        assert_eq!(
            ActivityFixture::recorded().rows(),
            [
                ActivityFixture::totals(TotalsWindow::Today, 1234, 42),
                ActivityFixture::totals(TotalsWindow::LastSevenDays, 6_588_481, 171),
            ]
        );
    }

    #[test]
    fn a_session_without_a_provider_names_no_model() {
        let output = r#"[{"provider":null,"model":"example-model","tokens_today":0,"cost_today":0,"tokens_last_seven_days":0,"cost_last_seven_days":0}]"#;

        let usage = OpencodeActivity::parse(output).unwrap().unwrap().usage();

        assert_eq!(usage.model(), None);
    }

    #[test]
    fn a_database_without_sessions_has_no_activity() {
        assert!(OpencodeActivity::parse("[]").unwrap().is_none());
    }

    #[test]
    fn empty_output_has_no_activity() {
        assert!(OpencodeActivity::parse("\n").unwrap().is_none());
    }

    #[test]
    fn output_that_is_not_a_json_table_is_unreadable() {
        assert_eq!(
            OpencodeActivity::parse("Error: no such table: session").unwrap_err(),
            UsageError::Unreadable
        );
    }
}

use std::iter::once;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::compact_count::CompactCount;
use super::compact_duration::CompactDuration;
use super::dialog::DialogStep;
use super::scroll_window::ScrollWindow;
use crate::application::{AgentReading, SessionService};
use crate::domain::{
    AgentUsage, LimitWindow, Spend, SpendKind, TokenTotals, TotalsWindow, UsageLimit, UsageRow,
};
use crate::ui::background_jobs::usage_feed::UsageFeed;
use crate::ui::theme::Theme;

pub struct UsageView {
    feed: UsageFeed,
    now: u64,
    scroll: ScrollWindow,
}

impl UsageView {
    pub const TITLE: &'static str = " Usage ";
    const CLOSE_HINT: &'static str = "u Esc q close";
    const FIRST_MEASUREMENT: &'static str = "Measuring usage…";
    const NO_REPORTING_AGENT: &'static str = "No agent reports its usage.";
    const NO_PLAN_USAGE: &'static str = "no plan usage reported";
    const MEASURING: &'static str = " · measuring";
    const INDENT: &'static str = "  ";
    const LABEL_WIDTH: usize = 16;
    const LONGEST_LABEL: usize = Self::LABEL_WIDTH - 1;
    const MINIMUM_GAP: usize = 2;
    const BAR_CELLS: usize = 10;
    const FILLED_CELL: &'static str = "█";
    const EMPTY_CELL: &'static str = "░";
    const FULL_PERCENT: usize = 100;

    pub fn open(service: &SessionService) -> Self {
        Self {
            feed: UsageFeed::open(service),
            now: service.now_epoch_seconds(),
            scroll: ScrollWindow::default(),
        }
    }

    pub fn poll(&mut self, now: Instant, service: &SessionService) {
        self.feed.poll(now, service);
        self.now = service.now_epoch_seconds();
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> DialogStep {
        if let KeyCode::Char('u' | 'q') | KeyCode::Esc = key.code {
            return DialogStep::Close;
        }
        self.scroll.scroll_for(key);
        DialogStep::Stay
    }

    pub fn lines(&self, width: usize, height: usize, theme: &Theme) -> Vec<Line<'static>> {
        let room = height.saturating_sub(1);
        let mut lines = self.scroll.window(self.body(theme), room);
        lines.push(self.footer(width, theme));
        lines
    }

    fn body(&self, theme: &Theme) -> Vec<Line<'static>> {
        let Some(reading) = self.feed.reading() else {
            return vec![Line::styled(Self::FIRST_MEASUREMENT, theme.notice())];
        };
        if reading.agents().is_empty() {
            return vec![Line::styled(Self::NO_REPORTING_AGENT, theme.notice())];
        }
        reading
            .agents()
            .iter()
            .flat_map(|agent| self.agent_block(agent, theme))
            .collect()
    }

    fn agent_block(&self, agent: &AgentReading, theme: &Theme) -> Vec<Line<'static>> {
        let heading = Self::heading(agent, theme);
        let rows = match agent.usage() {
            Ok(usage) => self.usage_rows(usage, theme),
            Err(error) => vec![Self::note(&error.to_string(), theme.error())],
        };
        once(heading)
            .chain(rows)
            .chain(once(Line::default()))
            .collect()
    }

    fn heading(agent: &AgentReading, theme: &Theme) -> Line<'static> {
        let name = Span::styled(agent.agent().to_string(), theme.usage_agent());
        match agent.usage().ok().and_then(AgentUsage::model) {
            Some(model) => Line::from(vec![name, Span::styled(format!(" · {model}"), theme.dim())]),
            None => Line::from(name),
        }
    }

    fn usage_rows(&self, usage: &AgentUsage, theme: &Theme) -> Vec<Line<'static>> {
        if usage.rows().is_empty() {
            return vec![Self::note(Self::NO_PLAN_USAGE, theme.notice())];
        }
        usage
            .rows()
            .iter()
            .map(|row| self.row(row, theme))
            .collect()
    }

    fn row(&self, row: &UsageRow, theme: &Theme) -> Line<'static> {
        match row {
            UsageRow::Limit(limit) => self.limit_row(limit, theme),
            UsageRow::Spend(spend) => Self::spend_row(spend, theme),
            UsageRow::Tokens(totals) => Self::tokens_row(totals, theme),
        }
    }

    fn limit_row(&self, limit: &UsageLimit, theme: &Theme) -> Line<'static> {
        let percent = limit.used_percent();
        Line::from(vec![
            Span::styled(
                Self::label(&Self::window_label(limit.window())),
                theme.base(),
            ),
            Span::styled(Self::bar(percent), theme.usage_bar()),
            Span::styled(format!(" {percent:>3}%"), theme.base()),
            Span::styled(self.reset(limit.resets_at()), theme.usage_reset()),
        ])
    }

    fn spend_row(spend: &Spend, theme: &Theme) -> Line<'static> {
        let amount = match spend.limit() {
            Some(limit) => format!("{} / {limit}", spend.used()),
            None => spend.used().to_string(),
        };
        Line::from(vec![
            Span::styled(Self::label(Self::spend_label(spend.kind())), theme.base()),
            Span::styled(amount, theme.base()),
        ])
    }

    fn tokens_row(totals: &TokenTotals, theme: &Theme) -> Line<'static> {
        let tokens = CompactCount::new(totals.tokens()).label();
        Line::from(vec![
            Span::styled(
                Self::label(Self::totals_label(totals.window())),
                theme.base(),
            ),
            Span::styled(format!("{tokens} tokens"), theme.base()),
            Span::styled(
                format!(" · {} at list price", totals.list_price()),
                theme.dim(),
            ),
        ])
    }

    fn window_label(window: &LimitWindow) -> String {
        match window {
            LimitWindow::Session => "Current session".to_string(),
            LimitWindow::Week => "This week".to_string(),
            LimitWindow::Scoped(name) => name.clone(),
            LimitWindow::Credits => "Credits".to_string(),
        }
    }

    fn totals_label(window: TotalsWindow) -> &'static str {
        match window {
            TotalsWindow::Today => "Today",
            TotalsWindow::LastSevenDays => "Last 7 days",
        }
    }

    fn spend_label(kind: SpendKind) -> &'static str {
        match kind {
            SpendKind::ExtraUsage => "Extra usage",
            SpendKind::Credits => "Credits used",
        }
    }

    fn label(text: &str) -> String {
        let width = Self::LABEL_WIDTH;
        let longest = Self::LONGEST_LABEL;
        format!("{}{text:<width$.longest$}", Self::INDENT)
    }

    fn bar(percent: u8) -> String {
        let half = Self::FULL_PERCENT / 2;
        let filled = (usize::from(percent) * Self::BAR_CELLS + half) / Self::FULL_PERCENT;
        let empty = Self::BAR_CELLS - filled;
        Self::FILLED_CELL.repeat(filled) + &Self::EMPTY_CELL.repeat(empty)
    }

    fn reset(&self, resets_at: Option<u64>) -> String {
        let Some(resets_at) = resets_at else {
            return String::new();
        };
        let remaining = CompactDuration::new(resets_at.saturating_sub(self.now));
        format!("  resets in {}", remaining.label())
    }

    fn note(text: &str, style: Style) -> Line<'static> {
        Line::styled(format!("{}{text}", Self::INDENT), style)
    }

    fn footer(&self, width: usize, theme: &Theme) -> Line<'static> {
        let status = self.status();
        let hint = self.scroll.hint(Self::CLOSE_HINT);
        let used = status.chars().count() + hint.chars().count();
        let gap = width.saturating_sub(used).max(Self::MINIMUM_GAP);
        let footer = format!("{status}{}{hint}", " ".repeat(gap));
        Line::styled(footer, theme.hint())
    }

    fn status(&self) -> String {
        let Some(reading) = self.feed.reading() else {
            return String::new();
        };
        let age = CompactDuration::new(self.now.saturating_sub(reading.measured_at()));
        let measuring = if self.feed.is_measuring() {
            Self::MEASURING
        } else {
            ""
        };
        format!("updated {} ago{measuring}", age.label())
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;

    use super::*;
    use crate::application::ports::UsageError;
    use crate::domain::Money;
    use crate::test_support::{Deadline, FakeUsageMeter, ServiceFixture};

    struct ViewFixture {
        services: ServiceFixture,
        service: SessionService,
    }

    impl ViewFixture {
        const WIDTH: usize = 60;
        const HEIGHT: usize = 20;
        const TWO_HOURS_FOURTEEN_MINUTES: u64 = 2 * 3600 + 14 * 60;

        fn with_meter(meter: FakeUsageMeter) -> Self {
            let services = ServiceFixture::new().with_usage_meter(meter);
            let service = services.service();
            Self { services, service }
        }

        fn reporting(rows: Vec<UsageRow>) -> Self {
            Self::with_meter(FakeUsageMeter::new().with_usage("claude", AgentUsage::new(rows)))
        }

        fn limit(window: LimitWindow, percent: f64, resets_in: Option<u64>) -> UsageRow {
            let resets_at = resets_in.map(|seconds| ServiceFixture::NOW + seconds);
            UsageRow::Limit(UsageLimit::used(window, percent, resets_at))
        }

        fn session_resetting_in(seconds: u64) -> Vec<String> {
            let row = Self::limit(LimitWindow::Session, 42.0, Some(seconds));
            Self::reporting(vec![row]).settled_rows()
        }

        fn totals(window: TotalsWindow) -> UsageRow {
            UsageRow::Tokens(TokenTotals::new(
                window,
                1234,
                Money::minor_units(42, "USD"),
            ))
        }

        fn extra_usage(used: u64, limit: Option<u64>) -> UsageRow {
            let spend = Spend::new(SpendKind::ExtraUsage, Money::minor_units(used, "USD"));
            UsageRow::Spend(match limit {
                Some(limit) => spend.with_limit(Money::minor_units(limit, "USD")),
                None => spend,
            })
        }

        fn settled(&self) -> UsageView {
            let mut view = UsageView::open(&self.service);
            let now = Instant::now();
            let settled = Deadline::for_background_work().wait_until(|| {
                view.poll(now, &self.service);
                !Self::text(&view).contains("measuring")
            });
            assert!(settled, "the measurement never finished");
            view
        }

        fn settled_rows(&self) -> Vec<String> {
            Self::rows(&self.settled())
        }

        fn rows(view: &UsageView) -> Vec<String> {
            Self::rows_within(view, Self::WIDTH, Self::HEIGHT)
        }

        fn rows_within(view: &UsageView, width: usize, height: usize) -> Vec<String> {
            view.lines(width, height, &Theme::default())
                .iter()
                .map(ToString::to_string)
                .collect()
        }

        fn text(view: &UsageView) -> String {
            Self::rows(view).join("\n").to_lowercase()
        }

        fn key(code: KeyCode) -> KeyEvent {
            KeyEvent::new(code, KeyModifiers::NONE)
        }
    }

    #[test]
    fn before_the_first_reading_the_view_says_it_is_measuring() {
        let fixture = ViewFixture::reporting(Vec::new());

        let view = UsageView::open(&fixture.service);

        assert_eq!(ViewFixture::rows(&view)[0], "Measuring usage…");
    }

    #[test]
    fn each_agent_gets_a_heading() {
        let rows = ViewFixture::session_resetting_in(ViewFixture::TWO_HOURS_FOURTEEN_MINUTES);

        assert_eq!(rows[0], "claude");
    }

    #[test]
    fn a_heading_names_the_model_the_agent_runs() {
        let usage = AgentUsage::new(Vec::new()).with_model("example-provider/example-model");
        let fixture = ViewFixture::with_meter(FakeUsageMeter::new().with_usage("claude", usage));

        assert_eq!(
            fixture.settled_rows()[0],
            "claude · example-provider/example-model"
        );
    }

    #[test]
    fn todays_totals_show_tokens_and_their_list_price() {
        let rows =
            ViewFixture::reporting(vec![ViewFixture::totals(TotalsWindow::Today)]).settled_rows();

        assert_eq!(
            rows[1],
            "  Today           1.2k tokens · $0.42 at list price"
        );
    }

    #[test]
    fn the_weeks_totals_cover_the_last_seven_days() {
        let rows = ViewFixture::reporting(vec![ViewFixture::totals(TotalsWindow::LastSevenDays)])
            .settled_rows();

        assert!(
            rows[1].starts_with("  Last 7 days     1.2k tokens"),
            "{}",
            rows[1]
        );
    }

    #[test]
    fn a_limit_shows_its_window_bar_percent_and_reset() {
        let rows = ViewFixture::session_resetting_in(ViewFixture::TWO_HOURS_FOURTEEN_MINUTES);

        assert_eq!(
            rows[1],
            "  Current session ████░░░░░░  42%  resets in 2h 14m"
        );
    }

    #[test]
    fn the_weekly_limit_is_this_week() {
        let rows = ViewFixture::reporting(vec![ViewFixture::limit(LimitWindow::Week, 18.0, None)])
            .settled_rows();

        assert_eq!(rows[1], "  This week       ██░░░░░░░░  18%");
    }

    #[test]
    fn a_scoped_limit_is_named_after_its_scope() {
        let window = LimitWindow::Scoped("Example Model".to_string());
        let rows =
            ViewFixture::reporting(vec![ViewFixture::limit(window, 5.0, None)]).settled_rows();

        assert_eq!(rows[1], "  Example Model   █░░░░░░░░░   5%");
    }

    #[test]
    fn a_long_scope_name_is_cut_short_of_the_bar() {
        let window = LimitWindow::Scoped("Example Model With A Long Name".to_string());
        let rows =
            ViewFixture::reporting(vec![ViewFixture::limit(window, 100.0, None)]).settled_rows();

        assert_eq!(rows[1], "  Example Model W ██████████ 100%");
    }

    #[test]
    fn an_unused_limit_has_an_empty_bar() {
        let rows = ViewFixture::reporting(vec![ViewFixture::limit(LimitWindow::Week, 0.0, None)])
            .settled_rows();

        assert_eq!(rows[1], "  This week       ░░░░░░░░░░   0%");
    }

    #[test]
    fn a_reset_within_the_hour_shows_minutes() {
        let rows = ViewFixture::session_resetting_in(45 * 60);

        assert!(rows[1].ends_with("resets in 45m"), "{}", rows[1]);
    }

    #[test]
    fn a_reset_within_the_minute_shows_under_a_minute() {
        let rows = ViewFixture::session_resetting_in(59);

        assert!(rows[1].ends_with("resets in <1m"), "{}", rows[1]);
    }

    #[test]
    fn a_reset_exactly_a_minute_away_shows_one_minute() {
        let rows = ViewFixture::session_resetting_in(60);

        assert!(rows[1].ends_with("resets in 1m"), "{}", rows[1]);
    }

    #[test]
    fn a_reset_days_away_shows_days_and_hours() {
        let rows = ViewFixture::session_resetting_in(3 * 86_400 + 4 * 3600 + 59);

        assert!(rows[1].ends_with("resets in 3d 4h"), "{}", rows[1]);
    }

    #[test]
    fn a_reset_exactly_a_day_away_shows_days() {
        let rows = ViewFixture::session_resetting_in(86_400);

        assert!(rows[1].ends_with("resets in 1d 0h"), "{}", rows[1]);
    }

    #[test]
    fn a_reset_exactly_an_hour_away_shows_hours() {
        let rows = ViewFixture::session_resetting_in(3600);

        assert!(rows[1].ends_with("resets in 1h 0m"), "{}", rows[1]);
    }

    #[test]
    fn a_credits_allowance_is_named_credits() {
        let rows =
            ViewFixture::reporting(vec![ViewFixture::limit(LimitWindow::Credits, 31.0, None)])
                .settled_rows();

        assert_eq!(rows[1], "  Credits         ███░░░░░░░  31%");
    }

    #[test]
    fn spent_credits_are_credits_used() {
        let spend = Spend::new(SpendKind::Credits, Money::minor_units(1860, "USD"))
            .with_limit(Money::minor_units(6000, "USD"));

        let rows = ViewFixture::reporting(vec![UsageRow::Spend(spend)]).settled_rows();

        assert_eq!(rows[1], "  Credits used    $18.60 / $60.00");
    }

    #[test]
    fn spend_shows_the_amount_used_out_of_its_limit() {
        let rows =
            ViewFixture::reporting(vec![ViewFixture::extra_usage(320, Some(5000))]).settled_rows();

        assert_eq!(rows[1], "  Extra usage     $3.20 / $50.00");
    }

    #[test]
    fn spend_without_a_limit_shows_the_amount_used() {
        let rows = ViewFixture::reporting(vec![ViewFixture::extra_usage(320, None)]).settled_rows();

        assert_eq!(rows[1], "  Extra usage     $3.20");
    }

    #[test]
    fn an_agent_without_plan_usage_says_so() {
        let rows = ViewFixture::reporting(Vec::new()).settled_rows();

        assert_eq!(rows[1], "  no plan usage reported");
    }

    #[test]
    fn an_agent_that_could_not_report_says_why() {
        let meter = FakeUsageMeter::new();
        meter.fail("claude");

        let rows = ViewFixture::with_meter(meter).settled_rows();

        assert_eq!(rows[1], format!("  {}", UsageError::Unanswered));
    }

    #[test]
    fn a_blank_line_separates_the_agents() {
        let rows = ViewFixture::reporting(Vec::new()).settled_rows();

        assert_eq!(rows[2], "");
    }

    #[test]
    fn with_no_agent_reporting_usage_the_view_says_so() {
        let rows = ViewFixture::with_meter(FakeUsageMeter::new()).settled_rows();

        assert_eq!(rows[0], "No agent reports its usage.");
    }

    #[test]
    fn the_footer_shows_the_age_of_the_reading_and_the_close_keys() {
        let fixture = ViewFixture::reporting(Vec::new());
        let mut view = fixture.settled();
        fixture.services.clock().advance(125);

        view.poll(Instant::now(), &fixture.service);

        let rows = ViewFixture::rows_within(&view, 40, ViewFixture::HEIGHT);
        assert_eq!(
            rows.last().unwrap(),
            "updated 2m ago             u Esc q close"
        );
    }

    #[test]
    fn a_fresh_reading_was_updated_under_a_minute_ago() {
        let rows = ViewFixture::reporting(Vec::new()).settled_rows();

        assert!(rows.last().unwrap().starts_with("updated <1m ago  "));
    }

    #[test]
    fn a_refresh_in_flight_is_shown_beside_the_age() {
        let fixture = ViewFixture::reporting(Vec::new());
        fixture.settled();
        fixture.services.clock().advance(300);
        let mut view = UsageView::open(&fixture.service);

        view.poll(Instant::now(), &fixture.service);

        assert!(
            ViewFixture::rows(&view)
                .last()
                .unwrap()
                .starts_with("updated 5m ago · measuring  ")
        );
    }

    #[test]
    fn a_narrow_footer_keeps_a_gap_before_the_close_keys() {
        let view = ViewFixture::reporting(Vec::new()).settled();

        let rows = ViewFixture::rows_within(&view, 10, ViewFixture::HEIGHT);

        assert_eq!(rows.last().unwrap(), "updated <1m ago  u Esc q close");
    }

    #[test]
    fn a_short_view_keeps_the_footer_below_the_rows_that_fit() {
        let view = ViewFixture::reporting(Vec::new()).settled();

        let rows = ViewFixture::rows_within(&view, ViewFixture::WIDTH, 2);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], "claude");
    }

    #[test]
    fn rows_that_do_not_fit_scroll_into_view() {
        let mut view = ViewFixture::reporting(Vec::new()).settled();
        ViewFixture::rows_within(&view, ViewFixture::WIDTH, 2);

        view.handle_key(&ViewFixture::key(KeyCode::Char('j')));

        let rows = ViewFixture::rows_within(&view, ViewFixture::WIDTH, 2);
        assert_eq!(rows[0], "  no plan usage reported");
    }

    #[test]
    fn rows_that_do_not_fit_offer_the_scroll_keys() {
        let view = ViewFixture::reporting(Vec::new()).settled();

        let rows = ViewFixture::rows_within(&view, ViewFixture::WIDTH, 2);

        assert!(
            rows[1].ends_with("↑ ↓ scroll · u Esc q close"),
            "{}",
            rows[1]
        );
    }

    #[test]
    fn rows_that_fit_offer_no_scroll_keys() {
        let rows = ViewFixture::reporting(Vec::new()).settled_rows();

        assert!(!rows.last().unwrap().contains("scroll"));
    }

    #[test]
    fn u_escape_and_q_close_the_view() {
        let mut view = ViewFixture::reporting(Vec::new()).settled();

        for code in [KeyCode::Char('u'), KeyCode::Esc, KeyCode::Char('q')] {
            assert_eq!(view.handle_key(&ViewFixture::key(code)), DialogStep::Close);
        }
    }

    #[test]
    fn another_key_keeps_the_view_open() {
        let mut view = ViewFixture::reporting(Vec::new()).settled();

        assert_eq!(
            view.handle_key(&ViewFixture::key(KeyCode::Char('j'))),
            DialogStep::Stay
        );
    }
}

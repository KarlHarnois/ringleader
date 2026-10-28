use std::mem;

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::agent_picker::AgentPicker;
use super::setting::Setting;
use crate::domain::{AgentMode, Checkout};
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct SettingsLine<'a> {
    mode: AgentMode,
    agents: &'a AgentPicker,
    checkout: Checkout,
}

impl<'a> SettingsLine<'a> {
    const INDENT: &'static str = "  ";
    const SEPARATOR: &'static str = " · ";
    const MARK_SEPARATOR: &'static str = ", ";
    const MODE_SUFFIX: &'static str = " mode";
    const NO_PLAN_MARK: &'static str = "no plan_args";
    const NO_PROMPT_MARK: &'static str = "no prompt";

    pub fn new(mode: AgentMode, agents: &'a AgentPicker, checkout: Checkout) -> Self {
        Self {
            mode,
            agents,
            checkout,
        }
    }

    pub fn lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let row_width = width.saturating_sub(Self::INDENT.len());
        let mut rows = Rows::new(row_width, Span::raw(Self::SEPARATOR));
        for setting in Setting::ALL {
            rows.add(self.item(setting, theme));
        }
        rows.into_lines().into_iter().map(Self::indented).collect()
    }

    fn indented(line: Line<'static>) -> Line<'static> {
        let mut spans = vec![Span::raw(Self::INDENT)];
        spans.extend(
            line.spans
                .into_iter()
                .map(|span| span.patch_style(line.style)),
        );
        Line::from(spans)
    }

    fn item(&self, setting: Setting, theme: &Theme) -> Item {
        let value = self.value(setting, theme);
        let style = value.style;
        let mut spans = vec![value];
        spans.extend(self.mark_spans(setting, theme));
        spans.push(Self::key_span(setting, theme));
        Item::new(spans, style)
    }

    fn value(&self, setting: Setting, theme: &Theme) -> Span<'static> {
        match setting {
            Setting::Mode => Span::styled(self.mode_text(), theme.base()),
            Setting::Agent => self.agent_value(theme),
            Setting::Checkout => self.checkout_value(theme),
        }
    }

    fn mode_text(&self) -> String {
        format!("{}{}", self.mode.name(), Self::MODE_SUFFIX)
    }

    fn agent_value(&self, theme: &Theme) -> Span<'static> {
        match self.agents.selected() {
            Ok(agent) => Span::styled(agent.name().to_string(), theme.base()),
            Err(problem) => Span::styled(problem, theme.error()),
        }
    }

    fn checkout_value(&self, theme: &Theme) -> Span<'static> {
        let style = match self.checkout {
            Checkout::NewWorktree => theme.base(),
            Checkout::Main => theme.warning(),
        };
        Span::styled(self.checkout.name(), style)
    }

    fn mark_spans(&self, setting: Setting, theme: &Theme) -> Vec<Span<'static>> {
        let Some(mark) = self.mark(setting) else {
            return Vec::new();
        };
        vec![
            Span::raw(Self::MARK_SEPARATOR),
            Span::styled(mark, theme.warning()),
        ]
    }

    fn mark(&self, setting: Setting) -> Option<&'static str> {
        let agent = self.agents.selected().ok()?;
        match setting {
            Setting::Mode => (!agent.supports(self.mode)).then_some(Self::NO_PLAN_MARK),
            Setting::Agent => (!agent.accepts_prompt()).then_some(Self::NO_PROMPT_MARK),
            Setting::Checkout => None,
        }
    }

    fn key_span(setting: Setting, theme: &Theme) -> Span<'static> {
        Span::styled(format!(" ({})", setting.key().label()), theme.dim())
    }
}

struct Item {
    spans: Vec<Span<'static>>,
    style: Style,
}

impl Item {
    fn new(spans: Vec<Span<'static>>, style: Style) -> Self {
        Self { spans, style }
    }

    fn width(&self) -> usize {
        self.spans.iter().map(Span::width).sum()
    }

    fn text(&self) -> String {
        self.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }
}

struct Rows {
    width: usize,
    separator: Span<'static>,
    closed: Vec<Line<'static>>,
    open: Vec<Span<'static>>,
    open_width: usize,
}

impl Rows {
    fn new(width: usize, separator: Span<'static>) -> Self {
        Self {
            width,
            separator,
            closed: Vec::new(),
            open: Vec::new(),
            open_width: 0,
        }
    }

    fn add(&mut self, item: Item) {
        if item.width() > self.width {
            self.add_wrapped(item);
        } else {
            self.pack(item);
        }
    }

    fn pack(&mut self, item: Item) {
        if !self.fits(&item) {
            self.close();
        }
        if !self.open.is_empty() {
            self.open_width += self.separator.width();
            self.open.push(self.separator.clone());
        }
        self.open_width += item.width();
        self.open.extend(item.spans);
    }

    fn fits(&self, item: &Item) -> bool {
        self.open.is_empty()
            || self.open_width + self.separator.width() + item.width() <= self.width
    }

    fn add_wrapped(&mut self, item: Item) {
        self.close();
        let wrapped = TextFit::new(&item.text()).styled_lines(self.width, item.style);
        self.closed.extend(wrapped);
    }

    fn close(&mut self) {
        if self.open.is_empty() {
            return;
        }
        self.open_width = 0;
        self.closed.push(Line::from(mem::take(&mut self.open)));
    }

    fn into_lines(mut self) -> Vec<Line<'static>> {
        self.close();
        self.closed
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::domain::Agent;
    use crate::test_support::ServiceFixture;

    struct LineFixture;

    impl LineFixture {
        const CONFIG_PATH: &'static str = "/config/ringleader/config.toml";
        const WIDTH: usize = 72;

        fn picker(agents: Vec<Agent>) -> AgentPicker {
            AgentPicker::new(&agents, Path::new(Self::CONFIG_PATH))
        }

        fn silent_agent() -> Agent {
            Agent::new("plain-agent", "plain-agent")
        }

        fn rows(line: &SettingsLine, width: usize) -> Vec<String> {
            line.lines(width, &Theme::default())
                .iter()
                .map(Self::text)
                .collect()
        }

        fn text(line: &Line<'static>) -> String {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        }

        fn in_style(line: &SettingsLine, style: Style) -> String {
            Self::in_style_at(line, Self::WIDTH, style)
        }

        fn in_style_at(line: &SettingsLine, width: usize, style: Style) -> String {
            line.lines(width, &Theme::default())
                .iter()
                .flat_map(|line| line.spans.clone())
                .filter(|span| span.style == style)
                .map(|span| span.content.to_string())
                .collect()
        }
    }

    #[test]
    fn the_items_run_mode_agent_checkout_with_their_keys() {
        let picker = LineFixture::picker(vec![ServiceFixture::agent()]);
        let line = SettingsLine::new(AgentMode::Plan, &picker, Checkout::NewWorktree);

        assert_eq!(
            LineFixture::rows(&line, LineFixture::WIDTH),
            ["  plan mode (Alt-j) · claude (Alt-k) · new worktree (Alt-l)"]
        );
    }

    #[test]
    fn plan_mode_is_marked_when_the_agent_has_no_plan_args() {
        let picker = LineFixture::picker(vec![LineFixture::silent_agent()]);
        let line = SettingsLine::new(AgentMode::Plan, &picker, Checkout::NewWorktree);

        assert!(
            LineFixture::rows(&line, LineFixture::WIDTH)[0]
                .starts_with("  plan mode, no plan_args (Alt-j)")
        );
    }

    #[test]
    fn build_mode_carries_no_mark_for_an_agent_without_plan_args() {
        let picker = LineFixture::picker(vec![LineFixture::silent_agent()]);
        let line = SettingsLine::new(AgentMode::Build, &picker, Checkout::NewWorktree);

        assert!(
            LineFixture::rows(&line, LineFixture::WIDTH)[0].starts_with("  build mode (Alt-j)")
        );
    }

    #[test]
    fn an_agent_without_a_prompt_placeholder_is_marked() {
        let picker = LineFixture::picker(vec![LineFixture::silent_agent()]);
        let line = SettingsLine::new(AgentMode::Build, &picker, Checkout::NewWorktree);

        assert!(
            LineFixture::rows(&line, LineFixture::WIDTH)[0]
                .ends_with("· plain-agent, no prompt (Alt-k)")
        );
    }

    #[test]
    fn the_marks_and_the_main_checkout_are_drawn_in_the_warning_color() {
        let picker = LineFixture::picker(vec![LineFixture::silent_agent()]);
        let line = SettingsLine::new(AgentMode::Plan, &picker, Checkout::Main);

        assert_eq!(
            LineFixture::in_style(&line, Theme::default().warning()),
            "no plan_argsno promptmain checkout"
        );
    }

    #[test]
    fn without_an_agent_the_config_path_error_takes_the_agent_slot_in_red() {
        let picker = LineFixture::picker(Vec::new());
        let line = SettingsLine::new(AgentMode::Build, &picker, Checkout::NewWorktree);

        assert_eq!(
            LineFixture::in_style(&line, Theme::default().error()),
            format!("no agents configured in {}", LineFixture::CONFIG_PATH)
        );
    }

    #[test]
    fn the_keys_are_dim() {
        let picker = LineFixture::picker(vec![ServiceFixture::agent()]);
        let line = SettingsLine::new(AgentMode::Plan, &picker, Checkout::NewWorktree);

        assert_eq!(
            LineFixture::in_style(&line, Theme::default().dim()),
            " (Alt-j) (Alt-k) (Alt-l)"
        );
    }

    #[test]
    fn a_line_wider_than_the_area_wraps_between_items() {
        let picker = LineFixture::picker(vec![ServiceFixture::agent()]);
        let line = SettingsLine::new(AgentMode::Plan, &picker, Checkout::NewWorktree);
        let compact_width = 54;

        assert_eq!(
            LineFixture::rows(&line, compact_width),
            [
                "  plan mode (Alt-j) · claude (Alt-k)",
                "  new worktree (Alt-l)"
            ]
        );
    }

    #[test]
    fn an_item_as_wide_as_the_row_keeps_its_styled_spans() {
        let picker = LineFixture::picker(Vec::new());
        let line = SettingsLine::new(AgentMode::Build, &picker, Checkout::NewWorktree);
        let error_item = format!(
            "no agents configured in {} (Alt-k)",
            LineFixture::CONFIG_PATH
        );
        let exact_width = SettingsLine::INDENT.len() + error_item.len();

        assert_eq!(
            LineFixture::in_style_at(&line, exact_width, Theme::default().dim()),
            " (Alt-j) (Alt-k) (Alt-l)"
        );
    }

    #[test]
    fn a_row_filled_to_its_last_column_keeps_both_items() {
        let picker = LineFixture::picker(vec![LineFixture::silent_agent()]);
        let line = SettingsLine::new(AgentMode::Plan, &picker, Checkout::NewWorktree);
        let second_row = "plain-agent, no prompt (Alt-k) · new worktree (Alt-l)";
        let exact_width = SettingsLine::INDENT.len() + second_row.chars().count();

        assert_eq!(
            LineFixture::rows(&line, exact_width),
            [
                "  plan mode, no plan_args (Alt-j)",
                "  plain-agent, no prompt (Alt-k) · new worktree (Alt-l)"
            ]
        );
    }

    #[test]
    fn an_item_wider_than_the_area_wraps_inside_itself() {
        let picker = LineFixture::picker(Vec::new());
        let line = SettingsLine::new(AgentMode::Build, &picker, Checkout::NewWorktree);
        let narrow_width = 40;

        let rows = LineFixture::rows(&line, narrow_width);

        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0], "  build mode (Alt-j)");
        assert_eq!(rows[1], "  no agents configured in");
        assert!(rows[2].ends_with("(Alt-k)"));
        assert_eq!(rows[3], "  new worktree (Alt-l)");
    }
}

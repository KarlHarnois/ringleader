use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::agent_picker::AgentPicker;
use super::new_session_field::NewSessionField;
use super::new_session_layout::NewSessionLayout;
use super::prompt_field::{PromptField, PromptStyle};
use super::repo_picker::RepoPicker;
use crate::domain::{AgentMode, Checkout};
use crate::ui::dialogs::dialog_labels::DialogLabels;
use crate::ui::dialogs::text_field::TextField;
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct NewSessionView<'a> {
    name: &'a TextField,
    repos: &'a RepoPicker,
    agents: &'a AgentPicker,
    prompt: &'a PromptField,
    focus: NewSessionField,
    checkout: Checkout,
    mode: AgentMode,
    error: Option<&'a str>,
}

impl<'a> NewSessionView<'a> {
    const CURSOR: char = '█';
    const NAME_ROWS: usize = 1;
    const CHECKOUT_LABEL: &'static str = "Checkout";
    const MODE_LABEL: &'static str = "Mode";
    const UNSUPPORTED_MODE_MARK: &'static str = " · this agent has no plan_args";

    pub fn new(
        name: &'a TextField,
        repos: &'a RepoPicker,
        agents: &'a AgentPicker,
        prompt: &'a PromptField,
    ) -> Self {
        Self {
            name,
            repos,
            agents,
            prompt,
            focus: NewSessionField::Name,
            checkout: Checkout::default(),
            mode: AgentMode::default(),
            error: None,
        }
    }

    pub fn focused_on(self, focus: NewSessionField) -> Self {
        Self { focus, ..self }
    }

    pub fn with_checkout(self, checkout: Checkout) -> Self {
        Self { checkout, ..self }
    }

    pub fn in_mode(self, mode: AgentMode) -> Self {
        Self { mode, ..self }
    }

    pub fn with_error(self, error: Option<&'a str>) -> Self {
        Self { error, ..self }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, labels: DialogLabels, theme: &Theme) {
        let errors = self.error_lines(usize::from(area.width), theme);
        let layout = self.layout(area, labels, errors.len(), theme);
        let upper = self.upper_lines(labels.field_width(area), layout.repo_room(), labels, theme);
        let [upper_area, pinned_area, prompt_area, error_area, hints_area] =
            layout.areas(upper.len());
        self.render_upper(frame, upper_area, upper);
        frame.render_widget(
            Paragraph::new(self.pinned_lines(labels, theme)),
            pinned_area,
        );
        self.prompt
            .render(frame, prompt_area, labels, self.prompt_style(theme));
        frame.render_widget(Paragraph::new(errors), error_area);
        frame.render_widget(Line::styled(self.hints(), theme.dim()), hints_area);
    }

    fn layout(
        &self,
        area: Rect,
        labels: DialogLabels,
        error_rows: usize,
        theme: &Theme,
    ) -> NewSessionLayout {
        let field_width = labels.field_width(area);
        let fixed_rows = Self::NAME_ROWS + self.agents.height(self.agents_expanded());
        NewSessionLayout::new(area, error_rows)
            .with_upper_rows(fixed_rows, self.repos.fewest_rows(field_width, theme))
            .with_prompt_rows(self.prompt.rows(field_width))
    }

    fn upper_lines(
        &self,
        field_width: usize,
        repo_room: usize,
        labels: DialogLabels,
        theme: &Theme,
    ) -> Vec<Line<'static>> {
        let cursor = (self.focus == NewSessionField::Name).then_some(Self::CURSOR);
        let name_line = Line::from(self.name.visible(field_width, cursor));
        let repo_style = self.focus_style(NewSessionField::Repo, theme);
        let repo_lines = self.repos.lines(field_width, repo_room, repo_style, theme);
        let agent_lines = self.agents.lines(self.agents_expanded(), theme);
        let mut lines = labels.labeled(NewSessionField::Name.label(), vec![name_line]);
        lines.extend(labels.labeled(NewSessionField::Repo.label(), repo_lines));
        lines.extend(labels.labeled(NewSessionField::Agent.label(), agent_lines));
        lines
    }

    fn pinned_lines(&self, labels: DialogLabels, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = self.checkout_lines(labels, theme);
        lines.extend(self.mode_lines(labels, theme));
        lines
    }

    fn checkout_lines(&self, labels: DialogLabels, theme: &Theme) -> Vec<Line<'static>> {
        let checkout_line = match self.checkout {
            Checkout::NewWorktree => Line::from(self.checkout.name()),
            Checkout::Main => Line::styled(self.checkout.name(), theme.warning()),
        };
        labels.labeled(Self::CHECKOUT_LABEL, vec![checkout_line])
    }

    fn mode_lines(&self, labels: DialogLabels, theme: &Theme) -> Vec<Line<'static>> {
        let mut mode_line = Line::from(self.mode.name());
        if self.selected_agent_lacks_the_mode() {
            mode_line.push_span(Span::styled(Self::UNSUPPORTED_MODE_MARK, theme.warning()));
        }
        labels.labeled(Self::MODE_LABEL, vec![mode_line])
    }

    fn selected_agent_lacks_the_mode(&self) -> bool {
        self.agents
            .selected()
            .is_ok_and(|agent| !agent.supports(self.mode))
    }

    fn render_upper(&self, frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>) {
        let rows_through_focus = self.rows_through_focus(lines.len());
        let hidden_rows = rows_through_focus.saturating_sub(usize::from(area.height));
        let scroll = u16::try_from(hidden_rows).unwrap_or(u16::MAX);
        frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), area);
    }

    fn rows_through_focus(&self, upper_rows: usize) -> usize {
        let agent_rows = self.agents.height(self.agents_expanded());
        let rows_above_agents = upper_rows.saturating_sub(agent_rows);
        match self.focus {
            NewSessionField::Name => Self::NAME_ROWS,
            NewSessionField::Repo => rows_above_agents,
            NewSessionField::Agent => rows_above_agents + self.agents.rows_through_selected(),
            NewSessionField::Prompt => 0,
        }
    }

    fn error_lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let wrapped = |error| TextFit::new(error).styled_lines(width, theme.error());
        self.error.map(wrapped).unwrap_or_default()
    }

    fn agents_expanded(&self) -> bool {
        self.focus == NewSessionField::Agent
    }

    fn focus_style(&self, field: NewSessionField, theme: &Theme) -> Style {
        if self.focus == field {
            theme.selected()
        } else {
            theme.base()
        }
    }

    fn prompt_style(&self, theme: &Theme) -> PromptStyle {
        if self.focus == NewSessionField::Prompt {
            PromptStyle::focused(theme)
        } else {
            PromptStyle::unfocused(theme)
        }
    }

    fn hints(&self) -> &'static str {
        self.repos.hints().unwrap_or_else(|| self.focus.hints())
    }
}

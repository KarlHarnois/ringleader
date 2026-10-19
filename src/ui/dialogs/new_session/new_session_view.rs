use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Padding, Paragraph};

use super::agent_picker::AgentPicker;
use super::new_session_field::NewSessionField;
use super::new_session_layout::NewSessionLayout;
use super::prompt_field::{PromptField, PromptStyle};
use super::repo_picker::RepoPicker;
use super::settings_line::SettingsLine;
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
    const CHEVRON: &'static str = "❯";
    const TEXT_INDENT: u16 = 2;
    const CHEVRON_ROWS: u16 = 1;

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
        let width = usize::from(area.width);
        let errors = self.error_lines(width, theme);
        let status = self.settings_line().lines(width, theme);
        let layout = self.layout(area, labels, errors.len(), status.len(), theme);
        let upper = self.upper_lines(labels.field_width(area), layout.repo_room(), labels, theme);
        let [upper_area, panel_area, status_area, error_area, hints_area] =
            layout.areas(upper.len());
        self.render_upper(frame, upper_area, upper);
        self.render_prompt_panel(frame, panel_area, theme);
        frame.render_widget(Paragraph::new(status), status_area);
        frame.render_widget(Paragraph::new(errors), error_area);
        frame.render_widget(Line::styled(self.hints(), theme.dim()), hints_area);
    }

    fn layout(
        &self,
        area: Rect,
        labels: DialogLabels,
        error_rows: usize,
        status_rows: usize,
        theme: &Theme,
    ) -> NewSessionLayout {
        let field_width = labels.field_width(area);
        NewSessionLayout::new(area, error_rows)
            .with_upper_rows(Self::NAME_ROWS, self.repos.fewest_rows(field_width, theme))
            .with_status_rows(status_rows)
            .with_prompt_rows(self.prompt.rows(self.prompt_text_width(area, theme)))
    }

    fn settings_line(&self) -> SettingsLine<'_> {
        SettingsLine::new(self.mode, self.agents, self.checkout)
    }

    fn upper_lines(
        &self,
        field_width: usize,
        repo_room: usize,
        labels: DialogLabels,
        theme: &Theme,
    ) -> Vec<Line<'static>> {
        let cursor = self
            .is_focused(NewSessionField::Name)
            .then_some(Self::CURSOR);
        let name_line = Line::from(self.name.visible(field_width, cursor));
        let repo_style = self.focus_style(NewSessionField::Repo, theme);
        let repo_lines = self.repos.lines(field_width, repo_room, repo_style, theme);
        let mut lines = labels.labeled(NewSessionField::Name.label(), vec![name_line]);
        lines.extend(labels.labeled(NewSessionField::Repo.label(), repo_lines));
        lines
    }

    fn render_upper(&self, frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>) {
        let hidden_rows = self.hidden_upper_rows(lines.len(), usize::from(area.height));
        let scroll = u16::try_from(hidden_rows).unwrap_or(u16::MAX);
        frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), area);
    }

    fn hidden_upper_rows(&self, upper_rows: usize, visible_rows: usize) -> usize {
        match self.focus {
            NewSessionField::Repo => upper_rows.saturating_sub(visible_rows),
            NewSessionField::Name | NewSessionField::Prompt => 0,
        }
    }

    fn render_prompt_panel(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let block = self.prompt_block(theme);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(
            Span::styled(Self::CHEVRON, self.rule_style(theme)),
            Self::chevron_area(area, inner),
        );
        self.prompt.render(frame, inner, self.prompt_style(theme));
    }

    fn chevron_area(panel: Rect, inner: Rect) -> Rect {
        Rect {
            x: panel.x,
            y: inner.y,
            width: Self::TEXT_INDENT,
            height: inner.height.min(Self::CHEVRON_ROWS),
        }
    }

    fn prompt_text_width(&self, area: Rect, theme: &Theme) -> usize {
        usize::from(self.prompt_block(theme).inner(area).width)
    }

    fn prompt_block(&self, theme: &Theme) -> Block<'static> {
        Block::new()
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_style(self.rule_style(theme))
            .padding(Padding::left(Self::TEXT_INDENT))
    }

    fn error_lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let wrapped = |error| TextFit::new(error).styled_lines(width, theme.error());
        self.error.map(wrapped).unwrap_or_default()
    }

    fn rule_style(&self, theme: &Theme) -> Style {
        if self.is_focused(NewSessionField::Prompt) {
            theme.base()
        } else {
            theme.dim()
        }
    }

    fn focus_style(&self, field: NewSessionField, theme: &Theme) -> Style {
        if self.is_focused(field) {
            theme.selected()
        } else {
            theme.base()
        }
    }

    fn prompt_style(&self, theme: &Theme) -> PromptStyle {
        if self.is_focused(NewSessionField::Prompt) {
            PromptStyle::focused(theme)
        } else {
            PromptStyle::unfocused(theme)
        }
    }

    fn is_focused(&self, field: NewSessionField) -> bool {
        self.focus == field
    }

    fn hints(&self) -> &'static str {
        self.repos.hints().unwrap_or_else(|| self.focus.hints())
    }
}

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use super::agent_picker::AgentPicker;
use super::dialog_labels::DialogLabels;
use super::new_session_field::NewSessionField;
use super::new_session_layout::NewSessionLayout;
use super::prompt_field::PromptField;
use super::repo_picker::RepoPicker;
use super::text_field::TextField;
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct NewSessionView<'a> {
    name: &'a TextField,
    repos: &'a RepoPicker,
    agents: &'a AgentPicker,
    prompt: &'a PromptField,
    focus: NewSessionField,
    error: Option<&'a str>,
}

impl<'a> NewSessionView<'a> {
    const CURSOR: char = '█';
    const NAME_ROWS: usize = 1;

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
            error: None,
        }
    }

    pub fn focused_on(self, focus: NewSessionField) -> Self {
        Self { focus, ..self }
    }

    pub fn with_error(self, error: Option<&'a str>) -> Self {
        Self { error, ..self }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, labels: DialogLabels, theme: &Theme) {
        let errors = self.error_lines(usize::from(area.width), theme);
        let layout = self.layout(area, labels, errors.len(), theme);
        let upper = self.upper_lines(labels.field_width(area), layout.repo_room(), labels, theme);
        let [upper_area, prompt_area, error_area, hints_area] = layout.areas(upper.len());
        frame.render_widget(Paragraph::new(upper), upper_area);
        let cursor_style = self.focus_style(NewSessionField::Prompt, theme);
        self.prompt.render(frame, prompt_area, labels, cursor_style);
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

    fn hints(&self) -> &'static str {
        self.repos.hints().unwrap_or_else(|| self.focus.hints())
    }
}

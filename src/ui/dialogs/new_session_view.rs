use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use super::agent_picker::AgentPicker;
use super::dialog_labels::DialogLabels;
use super::new_session_field::NewSessionField;
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
    error: &'a str,
}

impl<'a> NewSessionView<'a> {
    const CURSOR: char = '█';
    const FIXED_LINES: usize = 7;
    const PROMPT_ROWS: u16 = 1;

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
            error: "",
        }
    }

    pub fn focused_on(self, focus: NewSessionField) -> Self {
        Self { focus, ..self }
    }

    pub fn with_error(self, error: Option<&'a str>) -> Self {
        Self {
            error: error.unwrap_or_default(),
            ..self
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, labels: DialogLabels, theme: &Theme) {
        let field_width = labels.field_width(area);
        let upper = self.upper_lines(field_width, self.repo_room(area), labels, theme);
        let upper_rows = u16::try_from(upper.len()).unwrap_or(u16::MAX);
        let [upper_area, prompt_area, lower_area] = Layout::vertical([
            Constraint::Length(upper_rows),
            Constraint::Length(Self::PROMPT_ROWS),
            Constraint::Fill(1),
        ])
        .areas(area);
        frame.render_widget(Paragraph::new(upper), upper_area);
        let cursor_style = self.focus_style(NewSessionField::Prompt, theme);
        self.prompt.render(frame, prompt_area, labels, cursor_style);
        let lower = self.lower_lines(usize::from(area.width), theme);
        frame.render_widget(Paragraph::new(lower), lower_area);
    }

    fn repo_room(&self, area: Rect) -> usize {
        let fixed_lines = Self::FIXED_LINES + self.agents.height(self.agents_expanded());
        usize::from(area.height).saturating_sub(fixed_lines)
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

    fn lower_lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = vec![Line::from("")];
        lines.extend(TextFit::new(self.error).styled_lines(width, theme.error()));
        lines.push(Line::styled(self.hints(), theme.dim()));
        lines
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

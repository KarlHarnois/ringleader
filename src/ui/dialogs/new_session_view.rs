use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use super::agent_picker::AgentPicker;
use super::dialog_labels::DialogLabels;
use super::new_session_field::NewSessionField;
use super::repo_picker::RepoPicker;
use super::text_field::TextField;
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct NewSessionView<'a> {
    name: &'a TextField,
    repos: &'a RepoPicker,
    agents: &'a AgentPicker,
    prompt: &'a TextField,
    focus: NewSessionField,
    error: &'a str,
}

impl<'a> NewSessionView<'a> {
    const CURSOR: char = '█';
    const FIXED_LINES: usize = 7;

    pub fn new(
        name: &'a TextField,
        repos: &'a RepoPicker,
        agents: &'a AgentPicker,
        prompt: &'a TextField,
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
        let (width, height) = (usize::from(area.width), usize::from(area.height));
        let lines = self.lines(width, height, labels, theme);
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn lines(
        &self,
        width: usize,
        height: usize,
        labels: DialogLabels,
        theme: &Theme,
    ) -> Vec<Line<'static>> {
        let agents_expanded = self.focus == NewSessionField::Agent;
        let room = height.saturating_sub(Self::FIXED_LINES + self.agents.height(agents_expanded));
        let field_width = width.saturating_sub(labels.width());
        let repo_lines = self
            .repos
            .lines(field_width, room, self.repo_style(theme), theme);
        let agent_lines = self.agents.lines(agents_expanded, theme);
        let mut lines = vec![self.text_line(NewSessionField::Name, self.name, labels, width)];
        lines.extend(labels.labeled(NewSessionField::Repo.label(), repo_lines));
        lines.extend(labels.labeled(NewSessionField::Agent.label(), agent_lines));
        lines.push(self.text_line(NewSessionField::Prompt, self.prompt, labels, width));
        lines.push(Line::from(""));
        lines.extend(TextFit::new(self.error).styled_lines(width, theme.error()));
        lines.push(Line::styled(self.hints(), theme.dim()));
        lines
    }

    fn repo_style(&self, theme: &Theme) -> Style {
        if self.focus == NewSessionField::Repo {
            theme.selected()
        } else {
            theme.base()
        }
    }

    fn hints(&self) -> &'static str {
        self.repos.hints().unwrap_or_else(|| self.focus.hints())
    }

    fn text_line(
        &self,
        field: NewSessionField,
        text: &TextField,
        labels: DialogLabels,
        width: usize,
    ) -> Line<'static> {
        let cursor = (self.focus == field).then_some(Self::CURSOR);
        let shown = text.visible(width.saturating_sub(labels.width()), cursor);
        labels
            .labeled(field.label(), vec![Line::from(shown)])
            .remove(0)
    }
}

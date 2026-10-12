use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Style;
use ratatui::text::Line;

use super::agent_picker::AgentPicker;
use super::dialog::DialogStep;
use super::dialog_labels::DialogLabels;
use super::new_session_field::NewSessionField;
use super::repo_picker::{PickerStep, RepoPicker};
use super::text_field::{FieldKind, TextField};
use crate::application::{CreateRequest, SessionService};
use crate::ui::app::AppRequest;
use crate::ui::blocking_operation::BlockingOperation;
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct NewSessionDialog {
    name: TextField,
    repos: RepoPicker,
    agents: AgentPicker,
    prompt: TextField,
    focus: NewSessionField,
    error: Option<String>,
}

impl NewSessionDialog {
    pub const TITLE: &'static str = " New session ";
    const CREATE_KEY: char = 's';
    const CURSOR: char = '█';
    const FIXED_LINES: usize = 3;
    const PROMPT_ROWS: usize = 6;

    pub fn open(service: &SessionService) -> Self {
        Self {
            name: TextField::new(FieldKind::Name),
            repos: RepoPicker::open(service),
            agents: AgentPicker::new(service.config().agents(), &service.config_path()),
            prompt: TextField::new(FieldKind::Prompt),
            focus: NewSessionField::Name,
            error: None,
        }
    }

    pub fn poll(&mut self) {
        if self.repos.poll() == PickerStep::Picked {
            self.move_past_repo();
        }
    }

    pub fn fail(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn handle_key(&mut self, key: &KeyEvent, service: &SessionService) -> DialogStep {
        if self.focus == NewSessionField::Repo && self.picker_took(key, service) {
            return DialogStep::Stay;
        }
        if Self::creates(key) {
            return self.submit();
        }
        match key.code {
            KeyCode::Esc => return DialogStep::Close,
            KeyCode::Enter => return self.advance(),
            KeyCode::Tab => self.focus = self.focus.next(),
            KeyCode::BackTab => self.focus = self.focus.previous(),
            _ => self.edit_focused(key),
        }
        DialogStep::Stay
    }

    pub fn paste(&mut self, text: &str) {
        let pasted = match self.focus {
            NewSessionField::Name => self.name.paste(text),
            NewSessionField::Repo => self.repos.paste(text),
            NewSessionField::Prompt => self.prompt.paste(text),
            NewSessionField::Agent => Ok(()),
        };
        self.error = pasted.err();
    }

    pub fn lines(
        &self,
        width: usize,
        height: usize,
        labels: DialogLabels,
        theme: &Theme,
    ) -> Vec<Line<'static>> {
        let agents_expanded = self.focus == NewSessionField::Agent;
        let error_lines = self.error_lines(width, theme);
        let fixed_rows =
            Self::FIXED_LINES + self.agents.height(agents_expanded) + error_lines.len();
        let field_width = width.saturating_sub(labels.width());
        let free_rows = height.saturating_sub(fixed_rows);
        let (repo_lines, prompt_lines) = self.repo_and_prompt_lines(field_width, free_rows, theme);
        let agent_lines = self.agents.lines(agents_expanded, theme);
        let mut lines = vec![self.text_line(NewSessionField::Name, &self.name, labels, width)];
        lines.extend(labels.labeled(NewSessionField::Repo.label(), repo_lines));
        lines.extend(labels.labeled(NewSessionField::Agent.label(), agent_lines));
        lines.extend(labels.labeled(NewSessionField::Prompt.label(), prompt_lines));
        lines.push(Line::from(""));
        lines.extend(error_lines);
        lines.push(Line::styled(self.hints(), theme.dim()));
        lines
    }

    fn error_lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let error = self.error.as_deref().unwrap_or_default();
        TextFit::new(error).styled_lines(width, theme.error())
    }

    fn repo_and_prompt_lines(
        &self,
        width: usize,
        free_rows: usize,
        theme: &Theme,
    ) -> (Vec<Line<'static>>, Vec<Line<'static>>) {
        let repo_floor = self.repos.min_height(width, theme);
        let prompt_lines = self.prompt_lines(width, free_rows.saturating_sub(repo_floor));
        let room = free_rows.saturating_sub(prompt_lines.len());
        let repo_lines = self.repos.lines(width, room, self.repo_style(theme), theme);
        (repo_lines, prompt_lines)
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

    fn picker_took(&mut self, key: &KeyEvent, service: &SessionService) -> bool {
        match self.repos.handle_key(key, service) {
            PickerStep::Ignored => false,
            PickerStep::Handled => true,
            PickerStep::Picked => {
                self.move_past_repo();
                true
            }
        }
    }

    fn move_past_repo(&mut self) {
        self.focus = NewSessionField::Repo.next();
    }

    fn creates(key: &KeyEvent) -> bool {
        key.code == KeyCode::Char(Self::CREATE_KEY) && key.modifiers == KeyModifiers::CONTROL
    }

    fn advance(&mut self) -> DialogStep {
        if self.focus.is_last() {
            return self.submit();
        }
        self.focus = self.focus.next();
        DialogStep::Stay
    }

    fn text_line(
        &self,
        field: NewSessionField,
        text: &TextField,
        labels: DialogLabels,
        width: usize,
    ) -> Line<'static> {
        let cursor = self.cursor_on(field);
        let shown = text.visible(width.saturating_sub(labels.width()), cursor);
        labels
            .labeled(field.label(), vec![Line::from(shown)])
            .remove(0)
    }

    fn prompt_lines(&self, width: usize, spare_rows: usize) -> Vec<Line<'static>> {
        let rows = Self::PROMPT_ROWS.min(spare_rows).max(1);
        let cursor = self.cursor_on(NewSessionField::Prompt);
        let shown = self.prompt.visible_rows(width, rows, cursor);
        shown.into_iter().map(Line::from).collect()
    }

    fn cursor_on(&self, field: NewSessionField) -> Option<char> {
        (self.focus == field).then_some(Self::CURSOR)
    }

    fn edit_focused(&mut self, key: &KeyEvent) {
        match self.focus {
            NewSessionField::Name => self.name.edit(key),
            NewSessionField::Prompt => self.prompt.edit(key),
            NewSessionField::Agent => self.agents.handle_key(key),
            NewSessionField::Repo => false,
        };
    }

    fn submit(&mut self) -> DialogStep {
        match self.request() {
            Ok(request) => {
                DialogStep::Request(AppRequest::Block(BlockingOperation::Create(request)))
            }
            Err(problem) => {
                self.error = Some(problem);
                DialogStep::Stay
            }
        }
    }

    fn request(&self) -> Result<CreateRequest, String> {
        let name = self.name.value().trim();
        if name.is_empty() {
            return Err("enter a session name".to_string());
        }
        let repo = self.repos.selected_repo().ok_or("pick a repo")?;
        let agent = self.agents.selected()?;
        if let Some(refusal) = agent.prompt_refusal(self.prompt.value()) {
            return Err(refusal);
        }
        Ok(CreateRequest::new(name, repo.clone())
            .with_agent(agent.name())
            .with_prompt(self.prompt.value()))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    use crossterm::event::KeyModifiers;

    use super::*;
    use crate::domain::{Agent, Repo, RepoName};
    use crate::test_support::{FakeHostProbe, FakeRepoScanner, ServiceFixture};

    struct DialogFixture;

    impl DialogFixture {
        const SETTLE_LIMIT: Duration = Duration::from_secs(2);

        const FOUND: &'static str = "/home/example/work/api-gateway";
        const WORK: &'static str = "/home/example/work";
        const GATEWAY_COUNT: usize = 20;
        const TYPED: &'static str = "/srv/tools";
        const AGENT_HINTS: &'static str =
            "↑ ↓ j k choose · Enter next field · Ctrl-S create · Esc cancel";
        const PROMPT_HINTS: &'static str = "Ctrl-J new line · Enter create · Esc cancel";
        const SEARCH_HINTS: &'static str =
            "Type to search · ↑ ↓ ctrl-j ctrl-k choose · Enter pick · Esc back";
        const SEVEN_LINE_PROMPT: &'static str = "one\ntwo\nthree\nfour\nfive\nsix\nseven";
        const FULL_HEIGHT: usize = 20;
        const SHORT_HEIGHT: usize = 12;
        const REPO_AGENT_AND_ERROR_ROWS: usize = 3;
        const NO_PROMPT_ROOM_HEIGHT: usize =
            NewSessionDialog::FIXED_LINES + Self::REPO_AGENT_AND_ERROR_ROWS;

        fn launched_in_repo() -> ServiceFixture {
            let found = Repo::discovered(
                RepoName::parse("api-gateway").unwrap(),
                Path::new(Self::FOUND),
            );
            ServiceFixture::new().with_scanner(
                FakeRepoScanner::finding(vec![found]).inside_repo(Path::new(ServiceFixture::REPO)),
            )
        }

        fn launched_among_gateways() -> ServiceFixture {
            let found = (1..=Self::GATEWAY_COUNT)
                .map(|number| {
                    let name = format!("gateway-{number:02}");
                    let path = Path::new(Self::WORK).join(&name);
                    Repo::discovered(RepoName::parse(&name).unwrap(), &path)
                })
                .collect();
            ServiceFixture::new().with_scanner(
                FakeRepoScanner::finding(found).inside_repo(Path::new(ServiceFixture::REPO)),
            )
        }

        fn opened(fixture: &ServiceFixture) -> (NewSessionDialog, SessionService) {
            let service = fixture.service();
            let mut dialog = NewSessionDialog::open(&service);
            Self::settle(&mut dialog, |text| !text.contains("Finding the repo"));
            (dialog, service)
        }

        fn text(dialog: &NewSessionDialog) -> String {
            Self::text_at(dialog, Self::FULL_HEIGHT)
        }

        fn text_at(dialog: &NewSessionDialog, height: usize) -> String {
            dialog
                .lines(70, height, DialogLabels::full(), &Theme)
                .iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        }

        fn shown_at(dialog: &NewSessionDialog, height: usize) -> String {
            let text = Self::text_at(dialog, height);
            text.lines().take(height).collect::<Vec<_>>().join("\n")
        }

        fn flat_text(dialog: &NewSessionDialog) -> String {
            Self::text(dialog).replace('\n', " ")
        }

        fn settle(dialog: &mut NewSessionDialog, done: impl Fn(&str) -> bool) {
            let deadline = Instant::now() + Self::SETTLE_LIMIT;
            while Instant::now() < deadline && !done(&Self::text(dialog)) {
                dialog.poll();
                thread::sleep(Duration::from_millis(5));
            }
        }

        fn press(
            dialog: &mut NewSessionDialog,
            service: &SessionService,
            code: KeyCode,
        ) -> DialogStep {
            dialog.handle_key(&KeyEvent::new(code, KeyModifiers::NONE), service)
        }

        fn control(
            dialog: &mut NewSessionDialog,
            service: &SessionService,
            character: char,
        ) -> DialogStep {
            let key = KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL);
            dialog.handle_key(&key, service)
        }

        fn create(dialog: &mut NewSessionDialog, service: &SessionService) -> DialogStep {
            Self::control(dialog, service, 's')
        }

        fn in_prompt(fixture: &ServiceFixture) -> (NewSessionDialog, SessionService) {
            let (mut dialog, service) = Self::opened(fixture);
            Self::type_text(&mut dialog, &service, "fix-auth");
            Self::press(&mut dialog, &service, KeyCode::BackTab);
            (dialog, service)
        }

        fn hints(dialog: &NewSessionDialog) -> String {
            Self::text(dialog)
                .lines()
                .last()
                .unwrap_or_default()
                .to_string()
        }

        fn type_text(dialog: &mut NewSessionDialog, service: &SessionService, text: &str) {
            for character in text.chars() {
                Self::press(dialog, service, KeyCode::Char(character));
            }
        }

        fn search(dialog: &mut NewSessionDialog, service: &SessionService, query: &str) {
            Self::press(dialog, service, KeyCode::Tab);
            Self::type_text(dialog, service, query);
        }

        fn submitted(step: DialogStep) -> CreateRequest {
            match step {
                DialogStep::Request(AppRequest::Block(BlockingOperation::Create(request))) => {
                    request
                }
                other => panic!("expected a create request, got {other:?}"),
            }
        }
    }

    #[test]
    fn the_repo_field_shows_the_repo_ringleader_started_in() {
        let (dialog, _service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        assert!(DialogFixture::text(&dialog).contains("Repo      example-project\n"));
    }

    #[test]
    fn the_name_can_be_typed_while_the_home_search_runs() {
        let fixture =
            ServiceFixture::new().with_scanner(FakeRepoScanner::finding(Vec::new()).blocked());
        let service = fixture.service();
        let mut dialog = NewSessionDialog::open(&service);

        DialogFixture::type_text(&mut dialog, &service, "fix-auth");

        assert!(DialogFixture::text(&dialog).starts_with("Name      fix-auth█"));
        fixture.scanner().release();
    }

    #[test]
    fn picking_through_the_search_changes_the_repo() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");
        DialogFixture::search(&mut dialog, &service, "gate");
        DialogFixture::settle(&mut dialog, |text| text.contains("1/1 repos under ~"));

        DialogFixture::press(&mut dialog, &service, KeyCode::Enter);
        let shown = DialogFixture::text(&dialog);
        let step = DialogFixture::create(&mut dialog, &service);

        assert!(shown.contains("Repo      api-gateway\n"));
        assert_eq!(
            DialogFixture::submitted(step).repo().path(),
            Path::new(DialogFixture::FOUND)
        );
    }

    #[test]
    fn enter_walks_the_fields_then_creates_from_the_prompt() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");

        let walking = [KeyCode::Enter, KeyCode::Enter, KeyCode::Enter]
            .map(|code| DialogFixture::press(&mut dialog, &service, code));
        DialogFixture::type_text(&mut dialog, &service, "fix the tests");
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(
            walking,
            [DialogStep::Stay, DialogStep::Stay, DialogStep::Stay]
        );
        assert_eq!(
            DialogFixture::submitted(step),
            CreateRequest::new("fix-auth", ServiceFixture::repo())
                .with_agent("claude")
                .with_prompt("fix the tests")
        );
    }

    #[test]
    fn each_field_shows_its_own_hints() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        let mut hints = vec![DialogFixture::hints(&dialog)];
        for _ in 0..3 {
            DialogFixture::press(&mut dialog, &service, KeyCode::Enter);
            hints.push(DialogFixture::hints(&dialog));
        }

        assert_eq!(
            hints,
            [
                "Enter next field · Ctrl-S create · Esc cancel",
                "Type to find a repo · Enter next field · Ctrl-S create · Esc cancel",
                DialogFixture::AGENT_HINTS,
                DialogFixture::PROMPT_HINTS,
            ]
        );
    }

    #[test]
    fn picking_a_repo_moves_on_to_the_agent_field() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        DialogFixture::search(&mut dialog, &service, "gate");
        DialogFixture::settle(&mut dialog, |text| text.contains("1/1 repos under ~"));

        DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(DialogFixture::hints(&dialog), DialogFixture::AGENT_HINTS);
    }

    #[test]
    fn a_typed_repo_path_moves_on_to_the_agent_field_once_checked() {
        let fixture = DialogFixture::launched_in_repo()
            .with_host(FakeHostProbe::default().with_directory(Path::new(DialogFixture::TYPED)));
        let (mut dialog, service) = DialogFixture::opened(&fixture);
        DialogFixture::search(&mut dialog, &service, DialogFixture::TYPED);

        DialogFixture::press(&mut dialog, &service, KeyCode::Enter);
        DialogFixture::settle(&mut dialog, |text| {
            text.ends_with(DialogFixture::AGENT_HINTS)
        });

        assert!(DialogFixture::text(&dialog).contains("Repo      tools\n"));
        assert_eq!(DialogFixture::hints(&dialog), DialogFixture::AGENT_HINTS);
    }

    #[test]
    fn esc_in_the_search_closes_only_the_search() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        DialogFixture::search(&mut dialog, &service, "g");

        let first = DialogFixture::press(&mut dialog, &service, KeyCode::Esc);
        let second = DialogFixture::press(&mut dialog, &service, KeyCode::Esc);

        assert_eq!((first, second), (DialogStep::Stay, DialogStep::Close));
    }

    #[test]
    fn the_hints_follow_the_repo_field_and_its_search() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);
        let on_the_field = DialogFixture::text(&dialog);

        DialogFixture::type_text(&mut dialog, &service, "g");

        assert!(
            on_the_field
                .ends_with("Type to find a repo · Enter next field · Ctrl-S create · Esc cancel")
        );
        assert!(DialogFixture::text(&dialog).ends_with(DialogFixture::SEARCH_HINTS));
    }

    #[test]
    fn a_pasted_search_with_an_inner_line_break_is_a_field_error() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        DialogFixture::search(&mut dialog, &service, "g");

        dialog.paste("/work\n/other");

        assert!(DialogFixture::text(&dialog).contains("a path cannot contain a line break"));
    }

    #[test]
    fn without_a_repo_submitting_asks_for_one() {
        let (mut dialog, service) = DialogFixture::opened(&ServiceFixture::new());
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");

        let step = DialogFixture::create(&mut dialog, &service);

        assert_eq!(step, DialogStep::Stay);
        assert!(DialogFixture::text(&dialog).contains("pick a repo"));
    }

    #[test]
    fn j_and_k_move_through_the_agents() {
        let agents = vec![ServiceFixture::agent(), Agent::new("opencode", "opencode")];
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(agents));
        let (mut dialog, service) = DialogFixture::opened(&fixture);
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);

        DialogFixture::press(&mut dialog, &service, KeyCode::Char('j'));
        let after_j = DialogFixture::create(&mut dialog, &service);
        DialogFixture::press(&mut dialog, &service, KeyCode::Char('k'));
        let after_k = DialogFixture::create(&mut dialog, &service);

        assert_eq!(DialogFixture::submitted(after_j).agent(), Some("opencode"));
        assert_eq!(DialogFixture::submitted(after_k).agent(), Some("claude"));
    }

    #[test]
    fn ctrl_s_creates_from_the_name_field_with_the_defaults() {
        let fixture = DialogFixture::launched_in_repo();
        let (mut dialog, service) = DialogFixture::opened(&fixture);
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");

        let step = DialogFixture::create(&mut dialog, &service);

        assert_eq!(
            DialogFixture::submitted(step),
            CreateRequest::new("fix-auth", ServiceFixture::repo())
                .with_agent("claude")
                .with_prompt("")
        );
    }

    #[test]
    fn control_w_deletes_the_last_word_of_the_name() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        DialogFixture::type_text(&mut dialog, &service, "fix the auth");

        DialogFixture::control(&mut dialog, &service, 'w');

        assert!(DialogFixture::text(&dialog).starts_with("Name      fix the █"));
    }

    #[test]
    fn a_pasted_name_turns_line_breaks_into_spaces() {
        let fixture = DialogFixture::launched_in_repo();
        let (mut dialog, service) = DialogFixture::opened(&fixture);

        dialog.paste("fix\nauth");
        let step = DialogFixture::create(&mut dialog, &service);

        assert_eq!(DialogFixture::submitted(step).name(), "fix auth");
    }

    #[test]
    fn a_pasted_prompt_shows_its_line_breaks_and_keeps_them() {
        let (mut dialog, service) = DialogFixture::in_prompt(&DialogFixture::launched_in_repo());

        dialog.paste("one\r\ntwo");
        let shown = DialogFixture::text(&dialog);
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert!(shown.contains("Prompt    one\n          two█"));
        assert_eq!(DialogFixture::submitted(step).prompt(), "one\ntwo");
    }

    #[test]
    fn a_long_typed_prompt_wraps_under_its_label() {
        let (mut dialog, service) = DialogFixture::in_prompt(&DialogFixture::launched_in_repo());

        DialogFixture::type_text(
            &mut dialog,
            &service,
            "fix the flaky login test that times out on the first run after a cold start",
        );

        assert!(DialogFixture::text(&dialog).contains(
            "Prompt    fix the flaky login test that times out on the first run\n          after a cold start█"
        ));
    }

    #[test]
    fn control_j_in_the_prompt_adds_a_line_break_then_enter_creates() {
        let (mut dialog, service) = DialogFixture::in_prompt(&DialogFixture::launched_in_repo());

        DialogFixture::type_text(&mut dialog, &service, "one");
        DialogFixture::control(&mut dialog, &service, 'j');
        DialogFixture::type_text(&mut dialog, &service, "two");
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(DialogFixture::submitted(step).prompt(), "one\ntwo");
    }

    #[test]
    fn a_dialog_with_no_room_for_the_prompt_still_shows_one_prompt_row() {
        let (mut dialog, _service) = DialogFixture::in_prompt(&DialogFixture::launched_in_repo());

        dialog.paste("one\ntwo\nthree");

        let text = DialogFixture::text_at(&dialog, DialogFixture::NO_PROMPT_ROOM_HEIGHT);
        assert!(text.contains("Prompt    three█\n\n"));
    }

    #[test]
    fn a_tall_prompt_shows_its_last_six_rows() {
        let (mut dialog, _service) = DialogFixture::in_prompt(&DialogFixture::launched_in_repo());

        dialog.paste(DialogFixture::SEVEN_LINE_PROMPT);

        assert!(DialogFixture::text(&dialog).contains(
            "Prompt    two\n          three\n          four\n          five\n          six\n          seven█\n"
        ));
    }

    #[test]
    fn a_repo_search_shrinks_to_fit_beside_a_tall_prompt() {
        let fixture = DialogFixture::launched_among_gateways();
        let (mut dialog, service) = DialogFixture::in_prompt(&fixture);
        dialog.paste(DialogFixture::SEVEN_LINE_PROMPT);
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);

        DialogFixture::search(&mut dialog, &service, "gate");
        DialogFixture::settle(&mut dialog, |text| text.contains("20/20 repos under ~"));

        let shown = DialogFixture::shown_at(&dialog, DialogFixture::FULL_HEIGHT);
        assert!(shown.ends_with(DialogFixture::SEARCH_HINTS));
    }

    #[test]
    fn a_repo_search_under_a_tall_prompt_keeps_the_hints_in_view() {
        let (mut dialog, service) = DialogFixture::in_prompt(&DialogFixture::launched_in_repo());
        dialog.paste(DialogFixture::SEVEN_LINE_PROMPT);
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);

        DialogFixture::search(&mut dialog, &service, "gate");
        DialogFixture::settle(&mut dialog, |text| text.contains("1/1 repos under ~"));

        assert!(
            DialogFixture::shown_at(&dialog, DialogFixture::SHORT_HEIGHT)
                .ends_with(DialogFixture::SEARCH_HINTS)
        );
    }

    #[test]
    fn a_wrapped_error_under_a_tall_prompt_keeps_the_hints_in_view() {
        let (mut dialog, _service) = DialogFixture::in_prompt(&DialogFixture::launched_in_repo());
        dialog.paste(DialogFixture::SEVEN_LINE_PROMPT);

        dialog.fail(format!(
            "git worktree add failed: {}",
            "fatal: invalid reference ".repeat(6)
        ));

        assert!(
            DialogFixture::shown_at(&dialog, DialogFixture::SHORT_HEIGHT)
                .ends_with(DialogFixture::PROMPT_HINTS)
        );
    }

    #[test]
    fn a_prompt_for_an_agent_without_the_placeholder_is_refused() {
        let agent = Agent::new("plain-agent", "plain-agent");
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(vec![agent]));
        let (mut dialog, service) = DialogFixture::in_prompt(&fixture);
        DialogFixture::type_text(&mut dialog, &service, "fix the tests");

        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(step, DialogStep::Stay);
        assert!(
            DialogFixture::flat_text(&dialog).contains(
                "agent plain-agent cannot take a prompt, its prompt_args has no {prompt}"
            )
        );
    }

    #[test]
    fn no_agents_shows_the_config_path_and_cannot_be_submitted() {
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(Vec::new()));
        let (mut dialog, service) = DialogFixture::opened(&fixture);
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");

        let step = DialogFixture::create(&mut dialog, &service);

        assert_eq!(step, DialogStep::Stay);
        assert!(
            DialogFixture::text(&dialog)
                .contains("Agent     no agents configured in /config/ringleader/config.toml")
        );
    }

    #[test]
    fn the_agent_picker_lists_agents_in_config_order_and_marks_those_without_a_prompt() {
        let agents = vec![
            ServiceFixture::agent(),
            Agent::new("plain-agent", "plain-agent"),
        ];
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(agents));
        let (mut dialog, service) = DialogFixture::opened(&fixture);
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);

        DialogFixture::press(&mut dialog, &service, KeyCode::Down);
        let text = DialogFixture::text(&dialog);

        assert!(text.contains("Agent     claude\n          plain-agent · no prompt"));
    }

    #[test]
    fn a_failed_create_shows_its_error_in_the_dialog() {
        let fixture = DialogFixture::launched_in_repo();
        let (mut dialog, _service) = DialogFixture::opened(&fixture);

        dialog.fail("session fix-auth already exists".to_string());

        assert!(DialogFixture::text(&dialog).contains("session fix-auth already exists"));
    }
}

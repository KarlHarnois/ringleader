use crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::Line;

use super::agent_picker::AgentPicker;
use super::dialog::{DialogLabels, DialogStep};
use super::repo_picker::RepoPicker;
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
    focus: Field,
    error: Option<String>,
}

impl NewSessionDialog {
    pub const TITLE: &'static str = " New session ";
    const HINTS: &'static str = "Tab next field · Enter create · Esc cancel";
    const CURSOR: char = '█';
    const FIXED_LINES: usize = 7;

    pub fn open(service: &SessionService) -> Self {
        Self {
            name: TextField::new(FieldKind::Name),
            repos: RepoPicker::open(service),
            agents: AgentPicker::new(service.config().agents(), &service.config_path()),
            prompt: TextField::new(FieldKind::Prompt),
            focus: Field::Name,
            error: None,
        }
    }

    pub fn poll(&mut self, service: &mut SessionService) {
        self.repos.poll(service);
    }

    pub fn fail(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn handle_key(&mut self, key: &KeyEvent, service: &SessionService) -> DialogStep {
        let picker_has_focus = self.repos.has_open_form() || self.focus == Field::Repo;
        if picker_has_focus && self.repos.handle_key(key, service) {
            return DialogStep::Stay;
        }
        match key.code {
            KeyCode::Esc => return DialogStep::Close,
            KeyCode::Enter => return self.submit(),
            KeyCode::Tab => self.focus = self.focus.step(1),
            KeyCode::BackTab => self.focus = self.focus.step(Field::ORDER.len() - 1),
            _ => self.edit_focused(key),
        }
        DialogStep::Stay
    }

    pub fn paste(&mut self, text: &str) {
        if self.repos.has_open_form() {
            self.repos.paste(text);
            return;
        }
        let pasted = match self.focus {
            Field::Name => self.name.paste(text),
            Field::Prompt => self.prompt.paste(text),
            Field::Repo | Field::Agent => Ok(()),
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
        let room = height
            .saturating_sub(Self::FIXED_LINES + self.agents.height(self.focus == Field::Agent));
        let mut lines = vec![self.text_line(Field::Name, &self.name, labels, width)];
        lines.extend(labels.labeled(
            Field::Repo.label(),
            self.repo_lines(width, room, labels, theme),
        ));
        lines.extend(labels.labeled(
            Field::Agent.label(),
            self.agents.lines(self.focus == Field::Agent, theme),
        ));
        lines.push(self.text_line(Field::Prompt, &self.prompt, labels, width));
        lines.push(Line::from(""));
        let error = self.error.as_deref().unwrap_or_default();
        lines.extend(TextFit::new(error).styled_lines(width, theme.error()));
        lines.push(Line::styled(Self::HINTS, theme.dim()));
        lines
    }

    fn repo_lines(
        &self,
        width: usize,
        room: usize,
        labels: DialogLabels,
        theme: &Theme,
    ) -> Vec<Line<'static>> {
        if self.focus == Field::Repo || self.repos.has_open_form() {
            return self
                .repos
                .lines(width.saturating_sub(labels.width()), room, theme);
        }
        vec![Line::from(self.repos.summary())]
    }

    fn text_line(
        &self,
        field: Field,
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

    fn edit_focused(&mut self, key: &KeyEvent) {
        match self.focus {
            Field::Name => self.name.edit(key),
            Field::Prompt => self.prompt.edit(key),
            Field::Agent => self.agents.handle_key(key),
            Field::Repo => false,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Name,
    Repo,
    Agent,
    Prompt,
}

impl Field {
    const ORDER: [Field; 4] = [Field::Name, Field::Repo, Field::Agent, Field::Prompt];

    fn step(self, offset: usize) -> Self {
        let position = Self::ORDER.iter().position(|field| *field == self);
        Self::ORDER[(position.unwrap_or(0) + offset) % Self::ORDER.len()]
    }

    fn label(self) -> &'static str {
        match self {
            Field::Name => "Name",
            Field::Repo => "Repo",
            Field::Agent => "Agent",
            Field::Prompt => "Prompt",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    use crossterm::event::KeyModifiers;

    use super::*;
    use crate::application::ports::ScanError;
    use crate::domain::{Agent, Repo, RepoName};
    use crate::test_support::{FakeHostProbe, FakeRepoScanner, ServiceFixture};

    struct DialogFixture;

    impl DialogFixture {
        const SETTLE_LIMIT: Duration = Duration::from_secs(2);

        fn opened(fixture: &ServiceFixture) -> (NewSessionDialog, SessionService) {
            let mut service = fixture.service();
            let mut dialog = NewSessionDialog::open(&service);
            Self::settle(&mut dialog, &mut service, |text| !text.contains("Scanning"));
            (dialog, service)
        }

        fn text(dialog: &NewSessionDialog) -> String {
            dialog
                .lines(70, 20, DialogLabels::full(), &Theme)
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

        fn flat_text(dialog: &NewSessionDialog) -> String {
            Self::text(dialog).replace('\n', " ")
        }

        fn settle(
            dialog: &mut NewSessionDialog,
            service: &mut SessionService,
            done: impl Fn(&str) -> bool,
        ) {
            let deadline = Instant::now() + Self::SETTLE_LIMIT;
            while Instant::now() < deadline && !done(&Self::text(dialog)) {
                dialog.poll(service);
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

        fn type_text(dialog: &mut NewSessionDialog, service: &SessionService, text: &str) {
            for character in text.chars() {
                Self::press(dialog, service, KeyCode::Char(character));
            }
        }

        fn open_scope_input(dialog: &mut NewSessionDialog, service: &SessionService) {
            for code in [KeyCode::Tab, KeyCode::Down, KeyCode::Down, KeyCode::Enter] {
                Self::press(dialog, service, code);
            }
        }

        fn submitted(step: DialogStep) -> CreateRequest {
            match step {
                DialogStep::Request(AppRequest::Block(BlockingOperation::Create(request))) => {
                    request
                }
                other => panic!("expected a create request, got {other:?}"),
            }
        }

        fn with_repos(repos: Vec<Repo>) -> ServiceFixture {
            ServiceFixture::new().with_scanner(FakeRepoScanner::finding(repos))
        }

        fn repo(name: &str, path: &str) -> Repo {
            Repo::discovered(RepoName::parse(name).unwrap(), Path::new(path))
        }
    }

    #[test]
    fn the_picker_shows_scanning_until_the_scan_finishes() {
        let fixture = ServiceFixture::new()
            .with_scanner(FakeRepoScanner::finding(vec![ServiceFixture::repo()]).blocked());
        let mut service = fixture.service();
        let mut dialog = NewSessionDialog::open(&service);
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);
        let while_scanning = DialogFixture::text(&dialog);

        fixture.scanner().release();
        DialogFixture::settle(&mut dialog, &mut service, |text| text.contains("Scope:"));

        assert!(while_scanning.contains("Scanning /repos/example-project"));
        assert!(DialogFixture::text(&dialog).contains("> example-project"));
    }

    #[test]
    fn keys_keep_working_while_the_scan_runs() {
        let fixture =
            ServiceFixture::new().with_scanner(FakeRepoScanner::finding(Vec::new()).blocked());
        let service = fixture.service();
        let mut dialog = NewSessionDialog::open(&service);

        DialogFixture::type_text(&mut dialog, &service, "fix-auth");

        assert!(DialogFixture::text(&dialog).starts_with("Name    fix-auth"));
        fixture.scanner().release();
    }

    #[test]
    fn rows_that_share_a_name_show_their_canonical_paths() {
        let fixture = DialogFixture::with_repos(vec![
            DialogFixture::repo("example-project", "/repos/example-project"),
            DialogFixture::repo("example-project", "/work/example-project"),
        ]);
        let (mut dialog, service) = DialogFixture::opened(&fixture);

        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);
        let text = DialogFixture::text(&dialog);

        assert!(text.contains("example-project  /repos/example-project"));
        assert!(text.contains("example-project  /work/example-project"));
    }

    #[test]
    fn an_empty_scope_says_no_repos_were_found_and_offers_a_new_scope() {
        let fixture = DialogFixture::with_repos(Vec::new());
        let (mut dialog, service) = DialogFixture::opened(&fixture);

        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);
        let text = DialogFixture::text(&dialog);

        assert!(text.contains("no repos found in /repos/example-project"));
        assert!(text.contains("Change scope…"));
    }

    #[test]
    fn an_unreadable_scope_shows_the_error_and_offers_a_new_scope() {
        let fixture = ServiceFixture::new().with_scanner(
            FakeRepoScanner::finding(Vec::new()).failing(ScanError::Unreadable {
                path: Path::new("/repos/example-project").to_path_buf(),
                detail: "permission denied".to_string(),
            }),
        );
        let (mut dialog, service) = DialogFixture::opened(&fixture);

        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);
        let text = DialogFixture::text(&dialog);

        assert!(text.contains("cannot list /repos/example-project: permission denied"));
        assert!(text.contains("Change scope…"));
    }

    #[test]
    fn a_scan_cut_short_says_so_under_the_scope() {
        let fixture = ServiceFixture::new()
            .with_scanner(FakeRepoScanner::finding(vec![ServiceFixture::repo()]).cut_short());
        let (mut dialog, service) = DialogFixture::opened(&fixture);

        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);

        assert!(
            DialogFixture::text(&dialog)
                .contains("the scan of /repos/example-project was cut short")
        );
    }

    #[test]
    fn changing_the_scope_scans_the_typed_directory() {
        let fixture = ServiceFixture::new()
            .with_host(FakeHostProbe::default().with_directory(Path::new("/work/other")));
        let (mut dialog, mut service) = DialogFixture::opened(&fixture);
        DialogFixture::open_scope_input(&mut dialog, &service);

        DialogFixture::type_text(&mut dialog, &service, "/work/other");
        DialogFixture::press(&mut dialog, &service, KeyCode::Enter);
        DialogFixture::settle(&mut dialog, &mut service, |text| {
            text.contains("Scope: /work/other")
        });

        assert!(DialogFixture::text(&dialog).contains("Scope: /work/other"));
    }

    #[test]
    fn a_missing_scope_keeps_the_input_open_with_the_error_and_the_previous_list() {
        let fixture = ServiceFixture::new();
        let (mut dialog, mut service) = DialogFixture::opened(&fixture);
        DialogFixture::open_scope_input(&mut dialog, &service);

        DialogFixture::type_text(&mut dialog, &service, "/nowhere");
        DialogFixture::press(&mut dialog, &service, KeyCode::Enter);
        DialogFixture::settle(&mut dialog, &mut service, |text| {
            text.contains("no such directory")
        });

        let text = DialogFixture::text(&dialog);
        assert!(text.contains("no such directory: /nowhere"));
        assert!(text.contains("Scope: /repos/example-project"));
        assert!(text.contains("Scope path: /nowhere"));
    }

    #[test]
    fn leaving_the_scope_input_keeps_the_repo_chosen_before_it() {
        let fixture = ServiceFixture::new();
        let (mut dialog, service) = DialogFixture::opened(&fixture);
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");
        DialogFixture::open_scope_input(&mut dialog, &service);
        DialogFixture::press(&mut dialog, &service, KeyCode::Esc);
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);

        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(
            DialogFixture::submitted(step),
            CreateRequest::new("fix-auth", ServiceFixture::repo())
                .with_agent("claude")
                .with_prompt("")
        );
    }

    #[test]
    fn adding_a_repo_reloads_the_picker_and_selects_the_new_repo() {
        let fixture = ServiceFixture::new()
            .with_host(FakeHostProbe::default().with_directory(Path::new("/work/new-project")));
        let (mut dialog, mut service) = DialogFixture::opened(&fixture);
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");
        for code in [KeyCode::Tab, KeyCode::Down, KeyCode::Enter] {
            DialogFixture::press(&mut dialog, &service, code);
        }

        DialogFixture::type_text(&mut dialog, &service, "/work/new-project");
        DialogFixture::press(&mut dialog, &service, KeyCode::Enter);
        DialogFixture::settle(&mut dialog, &mut service, |text| {
            text.contains("> new-project · config")
        });
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(
            DialogFixture::submitted(step).repo().path(),
            Path::new("/work/new-project")
        );
        assert_eq!(service.config().repos().len(), 1);
    }

    #[test]
    fn esc_in_the_scope_input_closes_only_the_input() {
        let fixture = ServiceFixture::new();
        let (mut dialog, service) = DialogFixture::opened(&fixture);
        DialogFixture::open_scope_input(&mut dialog, &service);

        let first = DialogFixture::press(&mut dialog, &service, KeyCode::Esc);
        let second = DialogFixture::press(&mut dialog, &service, KeyCode::Esc);

        assert_eq!((first, second), (DialogStep::Stay, DialogStep::Close));
    }

    #[test]
    fn a_pasted_scope_with_an_inner_line_break_is_a_field_error() {
        let fixture = ServiceFixture::new();
        let (mut dialog, service) = DialogFixture::opened(&fixture);
        DialogFixture::open_scope_input(&mut dialog, &service);

        dialog.paste("/work\n/other");

        assert!(DialogFixture::text(&dialog).contains("a path cannot contain a line break"));
    }

    #[test]
    fn submitting_asks_for_the_selected_repo_with_the_first_agent() {
        let fixture = ServiceFixture::new();
        let (mut dialog, service) = DialogFixture::opened(&fixture);
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");

        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(
            DialogFixture::submitted(step),
            CreateRequest::new("fix-auth", ServiceFixture::repo())
                .with_agent("claude")
                .with_prompt("")
        );
    }

    #[test]
    fn a_pasted_name_turns_line_breaks_into_spaces() {
        let fixture = ServiceFixture::new();
        let (mut dialog, service) = DialogFixture::opened(&fixture);

        dialog.paste("fix\nauth");
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(DialogFixture::submitted(step).name(), "fix auth");
    }

    #[test]
    fn a_pasted_prompt_shows_its_line_breaks_and_keeps_them() {
        let fixture = ServiceFixture::new();
        let (mut dialog, service) = DialogFixture::opened(&fixture);
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");
        DialogFixture::press(&mut dialog, &service, KeyCode::BackTab);

        dialog.paste("one\r\ntwo");
        let shown = DialogFixture::text(&dialog);
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert!(shown.contains("Prompt  one⏎two█"));
        assert_eq!(DialogFixture::submitted(step).prompt(), "one\ntwo");
    }

    #[test]
    fn a_prompt_for_an_agent_without_the_placeholder_is_refused() {
        let agent = Agent::new("plain-agent", "plain-agent");
        let fixture =
            ServiceFixture::new().with_config(ServiceFixture::config().with_agents(vec![agent]));
        let (mut dialog, service) = DialogFixture::opened(&fixture);
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");
        DialogFixture::press(&mut dialog, &service, KeyCode::BackTab);
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
        let fixture =
            ServiceFixture::new().with_config(ServiceFixture::config().with_agents(Vec::new()));
        let (mut dialog, service) = DialogFixture::opened(&fixture);
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");

        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(step, DialogStep::Stay);
        assert!(
            DialogFixture::text(&dialog)
                .contains("Agent   no agents configured in /config/ringleader/config.toml")
        );
    }

    #[test]
    fn the_agent_picker_lists_agents_in_config_order_and_marks_those_without_a_prompt() {
        let agents = vec![
            ServiceFixture::agent(),
            Agent::new("plain-agent", "plain-agent"),
        ];
        let fixture =
            ServiceFixture::new().with_config(ServiceFixture::config().with_agents(agents));
        let (mut dialog, service) = DialogFixture::opened(&fixture);
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);

        DialogFixture::press(&mut dialog, &service, KeyCode::Down);
        let text = DialogFixture::text(&dialog);

        assert!(text.contains("Agent     claude\n          plain-agent · no prompt"));
    }

    #[test]
    fn a_failed_create_shows_its_error_in_the_dialog() {
        let fixture = ServiceFixture::new();
        let (mut dialog, _service) = DialogFixture::opened(&fixture);

        dialog.fail("session fix-auth already exists".to_string());

        assert!(DialogFixture::text(&dialog).contains("session fix-auth already exists"));
    }
}

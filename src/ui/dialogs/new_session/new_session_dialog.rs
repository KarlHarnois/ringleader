use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;

use super::agent_picker::AgentPicker;
use super::new_session_field::NewSessionField;
use super::new_session_view::NewSessionView;
use super::prompt_field::PromptField;
use super::repo_picker::{PickerStep, RepoPicker};
use crate::application::{CreateRequest, SessionService};
use crate::domain::{AgentMode, Checkout};
use crate::ui::app::AppRequest;
use crate::ui::blocking_operation::BlockingOperation;
use crate::ui::dialogs::DialogStep;
use crate::ui::dialogs::dialog_labels::DialogLabels;
use crate::ui::dialogs::text_field::{FieldKind, TextField};
use crate::ui::theme::Theme;

pub struct NewSessionDialog {
    name: TextField,
    repos: RepoPicker,
    agents: AgentPicker,
    prompt: PromptField,
    focus: NewSessionField,
    checkout: Checkout,
    mode: AgentMode,
    error: Option<String>,
}

impl NewSessionDialog {
    pub const TITLE: &'static str = " New session ";
    const CREATE_KEY: char = 's';
    const CHECKOUT_KEY: char = 't';
    const AGENT_KEY: char = 'p';

    pub fn open(service: &SessionService) -> Self {
        let agents = AgentPicker::new(service.config().agents(), &service.config_path());
        Self {
            name: TextField::new(FieldKind::Name),
            repos: RepoPicker::open(service),
            mode: Self::starting_mode(&agents),
            agents,
            prompt: PromptField::default(),
            focus: NewSessionField::Name,
            checkout: Checkout::default(),
            error: None,
        }
    }

    fn starting_mode(agents: &AgentPicker) -> AgentMode {
        let agent_has_plan_mode = agents
            .selected()
            .is_ok_and(|agent| agent.supports(AgentMode::Plan));
        if agent_has_plan_mode {
            AgentMode::Plan
        } else {
            AgentMode::Build
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
        if Self::toggles_mode(key) {
            self.mode = self.mode.toggled();
            return DialogStep::Stay;
        }
        if Self::toggles_checkout(key) {
            self.checkout = self.checkout.toggled();
            return DialogStep::Stay;
        }
        if Self::cycles_agent(key) {
            self.agents.cycle();
            return DialogStep::Stay;
        }
        if self.focus == NewSessionField::Repo && self.picker_took(key, service) {
            return DialogStep::Stay;
        }
        if Self::creates(key) {
            return self.submit();
        }
        self.handle_field_key(key)
    }

    pub fn paste(&mut self, text: &str) {
        let pasted = match self.focus {
            NewSessionField::Name => self.name.paste(text),
            NewSessionField::Repo => self.repos.paste(text),
            NewSessionField::Prompt => {
                self.prompt.paste(text);
                Ok(())
            }
        };
        self.error = pasted.err();
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, labels: DialogLabels, theme: &Theme) {
        NewSessionView::new(&self.name, &self.repos, &self.agents, &self.prompt)
            .focused_on(self.focus)
            .with_checkout(self.checkout)
            .in_mode(self.mode)
            .with_error(self.error.as_deref())
            .render(frame, area, labels, theme);
    }

    fn handle_field_key(&mut self, key: &KeyEvent) -> DialogStep {
        match key.code {
            KeyCode::Esc => return DialogStep::Close,
            KeyCode::Enter if self.prompt_swallows(key) => {}
            KeyCode::Enter => return self.advance(),
            KeyCode::Tab => self.focus = self.focus.next(),
            _ => self.edit_focused(key),
        }
        DialogStep::Stay
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

    fn toggles_mode(key: &KeyEvent) -> bool {
        key.code == KeyCode::BackTab
    }

    fn toggles_checkout(key: &KeyEvent) -> bool {
        Self::is_chord(key, KeyModifiers::CONTROL, Self::CHECKOUT_KEY)
    }

    fn cycles_agent(key: &KeyEvent) -> bool {
        Self::is_chord(key, KeyModifiers::ALT, Self::AGENT_KEY)
    }

    fn creates(key: &KeyEvent) -> bool {
        Self::is_chord(key, KeyModifiers::CONTROL, Self::CREATE_KEY)
    }

    fn is_chord(key: &KeyEvent, modifiers: KeyModifiers, character: char) -> bool {
        key.code == KeyCode::Char(character) && key.modifiers == modifiers
    }

    fn prompt_swallows(&self, enter: &KeyEvent) -> bool {
        self.focus == NewSessionField::Prompt && !enter.modifiers.is_empty()
    }

    fn advance(&mut self) -> DialogStep {
        if self.focus.is_last() {
            return self.submit();
        }
        self.focus = self.focus.next();
        DialogStep::Stay
    }

    fn edit_focused(&mut self, key: &KeyEvent) {
        match self.focus {
            NewSessionField::Name => {
                self.name.edit(key);
            }
            NewSessionField::Prompt => {
                self.prompt.edit(key);
            }
            NewSessionField::Repo => {}
        }
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
        let prompt = self.prompt.value();
        if let Some(refusal) = agent.launch_refusal(self.mode, &prompt) {
            return Err(refusal);
        }
        Ok(CreateRequest::new(name, repo.clone())
            .with_agent(agent.name())
            .with_checkout(self.checkout)
            .with_mode(self.mode)
            .with_prompt(&prompt))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    use crossterm::event::KeyModifiers;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::{Buffer, Cell};
    use ratatui::style::Modifier;

    use super::*;
    use crate::domain::{Agent, Repo, RepoName};
    use crate::test_support::{FakeHostProbe, FakeRepoScanner, ServiceFixture};

    struct DialogFixture;

    impl DialogFixture {
        const SETTLE_LIMIT: Duration = Duration::from_secs(2);
        const WIDTH: u16 = 74;
        const FULL_HEIGHT: u16 = 20;
        const COMPACT_HEIGHT: u16 = 10;
        const CURSOR: &'static str = "█";

        const FOUND: &'static str = "/home/example/work/api-gateway";
        const WORK: &'static str = "/home/example/work";
        const GATEWAY_COUNT: usize = 20;
        const TYPED: &'static str = "/srv/tools";
        const PROMPT_HINTS: &'static str =
            "Enter create · Alt-P agent · Shift-Tab mode · Ctrl-T checkout · Esc cancel";
        const SEARCH_HINTS: &'static str =
            "Type to search · ↑ ↓ Ctrl-J Ctrl-K choose · Enter pick · Esc back";
        const SEVEN_LINES: &'static str = "one\ntwo\nthree\nfour\nfive\nsix\nseven";

        fn launched_in_repo() -> ServiceFixture {
            let found = Repo::discovered(
                RepoName::parse("api-gateway").unwrap(),
                Path::new(Self::FOUND),
            );
            ServiceFixture::new().with_scanner(
                FakeRepoScanner::finding(vec![found]).inside_repo(Path::new(ServiceFixture::REPO)),
            )
        }

        fn opened(fixture: &ServiceFixture) -> (NewSessionDialog, SessionService) {
            let service = fixture.service();
            let mut dialog = NewSessionDialog::open(&service);
            Self::settle(&mut dialog, |text| !text.contains("Finding the repo"));
            (dialog, service)
        }

        fn launched_among_gateways() -> ServiceFixture {
            let found = (1..=Self::GATEWAY_COUNT)
                .map(|number| {
                    let name = format!("gateway-{number:02}");
                    Repo::discovered(
                        RepoName::parse(&name).unwrap(),
                        &Path::new(Self::WORK).join(&name),
                    )
                })
                .collect();
            ServiceFixture::new().with_scanner(
                FakeRepoScanner::finding(found).inside_repo(Path::new(ServiceFixture::REPO)),
            )
        }

        fn focused_on_prompt(fixture: &ServiceFixture) -> (NewSessionDialog, SessionService) {
            let (mut dialog, service) = Self::opened(fixture);
            Self::type_text(&mut dialog, &service, "fix-auth");
            Self::press(&mut dialog, &service, KeyCode::Tab);
            Self::press(&mut dialog, &service, KeyCode::Tab);
            (dialog, service)
        }

        fn opened_with_agents(agent_count: usize) -> (NewSessionDialog, SessionService) {
            let agents = (1..=agent_count)
                .map(|number| Agent::new(&format!("agent-{number}"), "sh"))
                .collect();
            let fixture =
                Self::launched_in_repo().with_config(ServiceFixture::config().with_agents(agents));
            Self::opened(&fixture)
        }

        fn launched_with_two_agents() -> ServiceFixture {
            let opencode = Agent::new("opencode", "opencode")
                .with_prompt_args(&["{prompt}".to_string()])
                .with_plan_args(&["--agent".to_string(), "plan".to_string()]);
            let agents = vec![ServiceFixture::agent(), opencode];
            Self::launched_in_repo().with_config(ServiceFixture::config().with_agents(agents))
        }

        fn text(dialog: &NewSessionDialog) -> String {
            Self::text_at(dialog, Self::FULL_HEIGHT)
        }

        fn text_at(dialog: &NewSessionDialog, height: u16) -> String {
            Self::rows(&Self::screen(dialog, height), |cell| cell.symbol())
        }

        fn text_with_cursor(dialog: &NewSessionDialog) -> String {
            let screen = Self::screen(dialog, Self::FULL_HEIGHT);
            Self::rows(&screen, |cell| {
                if Self::is_cursor(cell) {
                    Self::CURSOR
                } else {
                    cell.symbol()
                }
            })
        }

        fn shows_a_cursor(dialog: &NewSessionDialog) -> bool {
            let screen = Self::screen(dialog, Self::FULL_HEIGHT);
            screen.content().iter().any(Self::is_cursor)
        }

        fn underlined(dialog: &NewSessionDialog) -> String {
            let screen = Self::screen(dialog, Self::FULL_HEIGHT);
            screen
                .content()
                .iter()
                .filter(|cell| cell.modifier.contains(Modifier::UNDERLINED))
                .map(Cell::symbol)
                .collect()
        }

        fn in_warning_color(dialog: &NewSessionDialog) -> String {
            let warning = Theme.warning().fg;
            let screen = Self::screen(dialog, Self::FULL_HEIGHT);
            screen
                .content()
                .iter()
                .filter(|cell| Some(cell.fg) == warning)
                .map(Cell::symbol)
                .collect()
        }

        fn is_cursor(cell: &Cell) -> bool {
            cell.modifier.contains(Modifier::REVERSED)
        }

        fn screen(dialog: &NewSessionDialog, height: u16) -> Buffer {
            let mut terminal = Terminal::new(TestBackend::new(Self::WIDTH, height)).unwrap();
            terminal
                .draw(|frame| dialog.render(frame, frame.area(), DialogLabels::full(), &Theme))
                .unwrap();
            terminal.backend().buffer().clone()
        }

        fn rows<'a>(screen: &'a Buffer, shown: impl Fn(&'a Cell) -> &'a str) -> String {
            let area = screen.area;
            let rows: Vec<String> = (area.top()..area.bottom())
                .map(|row| {
                    let cells = (area.left()..area.right()).map(|column| &screen[(column, row)]);
                    cells.map(&shown).collect::<String>().trim_end().to_string()
                })
                .collect();
            rows.join("\n").trim_end().to_string()
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
            Self::chord(dialog, service, KeyModifiers::CONTROL, character)
        }

        fn chord(
            dialog: &mut NewSessionDialog,
            service: &SessionService,
            modifiers: KeyModifiers,
            character: char,
        ) -> DialogStep {
            let key = KeyEvent::new(KeyCode::Char(character), modifiers);
            dialog.handle_key(&key, service)
        }

        fn create(dialog: &mut NewSessionDialog, service: &SessionService) -> DialogStep {
            Self::control(dialog, service, 's')
        }

        fn cycle_agent(dialog: &mut NewSessionDialog, service: &SessionService) -> DialogStep {
            Self::chord(dialog, service, KeyModifiers::ALT, 'p')
        }

        fn worktree_error(repeats: usize) -> String {
            let reasons = vec!["fatal: invalid reference"; repeats].join(" ");
            format!("git worktree add failed: {reasons}")
        }

        fn enter_with(
            dialog: &mut NewSessionDialog,
            service: &SessionService,
            modifiers: KeyModifiers,
        ) -> DialogStep {
            dialog.handle_key(&KeyEvent::new(KeyCode::Enter, modifiers), service)
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

        fn type_lines(dialog: &mut NewSessionDialog, service: &SessionService, lines: &[&str]) {
            for (index, line) in lines.iter().enumerate() {
                if index > 0 {
                    Self::control(dialog, service, 'j');
                }
                Self::type_text(dialog, service, line);
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

        let walking = [KeyCode::Enter, KeyCode::Enter]
            .map(|code| DialogFixture::press(&mut dialog, &service, code));
        DialogFixture::type_text(&mut dialog, &service, "fix the tests");
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(walking, [DialogStep::Stay, DialogStep::Stay]);
        assert_eq!(
            DialogFixture::submitted(step),
            CreateRequest::new("fix-auth", ServiceFixture::repo())
                .with_agent("claude")
                .with_mode(AgentMode::Plan)
                .with_prompt("fix the tests")
        );
    }

    #[test]
    fn each_field_shows_its_own_hints() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        let mut hints = vec![DialogFixture::hints(&dialog)];
        for _ in 0..2 {
            DialogFixture::press(&mut dialog, &service, KeyCode::Enter);
            hints.push(DialogFixture::hints(&dialog));
        }

        assert_eq!(
            hints,
            [
                "Enter next field · Ctrl-S create · Shift-Tab mode · Esc cancel",
                "Type to find a repo · Enter next field · Ctrl-S create · Esc cancel",
                DialogFixture::PROMPT_HINTS,
            ]
        );
    }

    #[test]
    fn picking_a_repo_moves_on_to_the_prompt() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        DialogFixture::search(&mut dialog, &service, "gate");
        DialogFixture::settle(&mut dialog, |text| text.contains("1/1 repos under ~"));

        DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(DialogFixture::hints(&dialog), DialogFixture::PROMPT_HINTS);
    }

    #[test]
    fn a_typed_repo_path_moves_on_to_the_prompt_once_checked() {
        let fixture = DialogFixture::launched_in_repo()
            .with_host(FakeHostProbe::default().with_directory(Path::new(DialogFixture::TYPED)));
        let (mut dialog, service) = DialogFixture::opened(&fixture);
        DialogFixture::search(&mut dialog, &service, DialogFixture::TYPED);

        DialogFixture::press(&mut dialog, &service, KeyCode::Enter);
        DialogFixture::settle(&mut dialog, |text| {
            text.ends_with(DialogFixture::PROMPT_HINTS)
        });

        assert!(DialogFixture::text(&dialog).contains("Repo      tools\n"));
        assert_eq!(DialogFixture::hints(&dialog), DialogFixture::PROMPT_HINTS);
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
    fn alt_p_picks_the_next_agent() {
        let (mut dialog, service) =
            DialogFixture::opened(&DialogFixture::launched_with_two_agents());
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");

        DialogFixture::cycle_agent(&mut dialog, &service);
        let step = DialogFixture::create(&mut dialog, &service);

        assert_eq!(DialogFixture::submitted(step).agent(), Some("opencode"));
    }

    #[test]
    fn alt_p_past_the_last_agent_wraps_to_the_first() {
        let (mut dialog, service) =
            DialogFixture::opened(&DialogFixture::launched_with_two_agents());
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");

        DialogFixture::cycle_agent(&mut dialog, &service);
        DialogFixture::cycle_agent(&mut dialog, &service);
        let step = DialogFixture::create(&mut dialog, &service);

        assert_eq!(DialogFixture::submitted(step).agent(), Some("claude"));
    }

    #[test]
    fn alt_p_shows_the_next_agent_on_the_pinned_row() {
        let (mut dialog, service) =
            DialogFixture::opened(&DialogFixture::launched_with_two_agents());

        DialogFixture::cycle_agent(&mut dialog, &service);

        assert!(
            DialogFixture::text(&dialog).contains("Agent     opencode\nCheckout  new worktree\n")
        );
    }

    #[test]
    fn alt_p_keeps_the_dialog_open_on_the_focused_field_and_types_nothing() {
        let (mut dialog, service) =
            DialogFixture::opened(&DialogFixture::launched_with_two_agents());
        DialogFixture::type_text(&mut dialog, &service, "ship");

        let step = DialogFixture::cycle_agent(&mut dialog, &service);

        assert_eq!(step, DialogStep::Stay);
        assert!(DialogFixture::text(&dialog).starts_with("Name      ship█\n"));
        assert_eq!(DialogFixture::hints(&dialog), NewSessionField::Name.hints());
    }

    #[test]
    fn alt_p_switches_the_agent_without_typing_into_the_prompt() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_with_two_agents());
        DialogFixture::type_text(&mut dialog, &service, "fix");

        DialogFixture::cycle_agent(&mut dialog, &service);
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        let request = DialogFixture::submitted(step);
        assert_eq!(request.prompt(), "fix");
        assert_eq!(request.agent(), Some("opencode"));
    }

    #[test]
    fn alt_p_switches_the_agent_while_the_repo_search_is_open() {
        let (mut dialog, service) =
            DialogFixture::opened(&DialogFixture::launched_with_two_agents());
        DialogFixture::search(&mut dialog, &service, "gate");

        DialogFixture::cycle_agent(&mut dialog, &service);

        let shown = DialogFixture::text(&dialog);
        assert!(shown.contains("Find: gate"));
        assert!(shown.contains("Agent     opencode\n"));
    }

    #[test]
    fn alt_p_with_no_agents_keeps_the_config_path_error() {
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(Vec::new()));
        let (mut dialog, service) = DialogFixture::opened(&fixture);

        DialogFixture::cycle_agent(&mut dialog, &service);

        assert!(
            DialogFixture::text(&dialog)
                .contains("Agent     no agents configured in /config/ringleader/config.toml")
        );
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
                .with_mode(AgentMode::Plan)
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
    fn a_pasted_prompt_turns_every_kind_of_line_break_into_a_newline() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());

        dialog.paste("one\r\ntwo\rthree");
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(DialogFixture::submitted(step).prompt(), "one\ntwo\nthree");
    }

    #[test]
    fn a_pasted_prompt_shows_each_line_on_its_own_row() {
        let (mut dialog, _service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());

        dialog.paste("one\ntwo\nthree");

        assert!(
            DialogFixture::text_with_cursor(&dialog)
                .contains("Prompt    one\n          two\n          three█")
        );
    }

    #[test]
    fn a_pasted_prompt_keeps_the_indent_of_each_line() {
        let (mut dialog, _service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());

        dialog.paste("steps:\n  - run: cargo test");

        assert!(
            DialogFixture::text(&dialog)
                .contains("Prompt    steps:\n            - run: cargo test")
        );
    }

    #[test]
    fn a_long_typed_prompt_wraps_under_its_label_and_submits_as_one_line() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        let prompt = "fix the flaky login test that times out on the first run after a cold start";

        DialogFixture::type_text(&mut dialog, &service, prompt);
        let shown = DialogFixture::text_with_cursor(&dialog);
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert!(shown.contains(
            "Prompt    fix the flaky login test that times out on the first run after\n          a cold start█"
        ));
        assert_eq!(DialogFixture::submitted(step).prompt(), prompt);
    }

    #[test]
    fn the_cursor_stays_in_view_as_the_prompt_reaches_the_right_edge() {
        let field_width = usize::from(DialogFixture::WIDTH) - DialogLabels::full().width();

        let shown = [field_width - 1, field_width].map(|length| {
            let (mut dialog, service) =
                DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
            DialogFixture::type_text(&mut dialog, &service, &"x".repeat(length));
            DialogFixture::shows_a_cursor(&dialog)
        });

        assert_eq!(shown, [true, true]);
    }

    #[test]
    fn control_j_in_the_prompt_adds_a_line_break_then_enter_creates() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());

        DialogFixture::type_text(&mut dialog, &service, "one");
        DialogFixture::control(&mut dialog, &service, 'j');
        DialogFixture::type_text(&mut dialog, &service, "two");
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(DialogFixture::submitted(step).prompt(), "one\ntwo");
    }

    #[test]
    fn control_u_at_the_end_of_a_prompt_line_clears_only_that_line() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        DialogFixture::type_lines(&mut dialog, &service, &["first line", "second"]);

        DialogFixture::control(&mut dialog, &service, 'u');

        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);
        assert_eq!(DialogFixture::submitted(step).prompt(), "first line\n");
    }

    #[test]
    fn control_u_in_the_middle_of_a_prompt_line_keeps_the_text_after_the_cursor() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        DialogFixture::type_lines(&mut dialog, &service, &["first line", "fix the tests"]);
        for _ in 0.."tests".len() {
            DialogFixture::press(&mut dialog, &service, KeyCode::Left);
        }

        DialogFixture::control(&mut dialog, &service, 'u');

        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);
        assert_eq!(DialogFixture::submitted(step).prompt(), "first line\ntests");
    }

    #[test]
    fn control_u_at_the_start_of_a_prompt_line_joins_it_to_the_line_above() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        DialogFixture::type_lines(&mut dialog, &service, &["first line", "second"]);
        DialogFixture::press(&mut dialog, &service, KeyCode::Home);

        DialogFixture::control(&mut dialog, &service, 'u');

        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);
        assert_eq!(DialogFixture::submitted(step).prompt(), "first linesecond");
    }

    #[test]
    fn control_w_in_the_prompt_deletes_the_last_word_of_its_line() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        DialogFixture::type_lines(&mut dialog, &service, &["first line", "fix the tests"]);

        DialogFixture::control(&mut dialog, &service, 'w');

        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);
        assert_eq!(
            DialogFixture::submitted(step).prompt(),
            "first line\nfix the "
        );
    }

    #[test]
    fn shift_or_alt_enter_in_the_prompt_creates_nothing_and_adds_nothing() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        DialogFixture::type_text(&mut dialog, &service, "fix the flaky");

        let steps = [KeyModifiers::SHIFT, KeyModifiers::ALT]
            .map(|modifiers| DialogFixture::enter_with(&mut dialog, &service, modifiers));
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(steps, [DialogStep::Stay, DialogStep::Stay]);
        assert_eq!(DialogFixture::submitted(step).prompt(), "fix the flaky");
    }

    #[test]
    fn tab_leaves_the_prompt_without_typing_into_it() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        DialogFixture::type_text(&mut dialog, &service, "fix");

        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);
        let step = DialogFixture::create(&mut dialog, &service);

        assert_eq!(
            DialogFixture::hints(&dialog),
            "Enter next field · Ctrl-S create · Shift-Tab mode · Esc cancel"
        );
        assert_eq!(DialogFixture::submitted(step).prompt(), "fix");
    }

    #[test]
    fn shift_tab_switches_the_mode_without_typing_into_the_prompt() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        DialogFixture::type_text(&mut dialog, &service, "fix");

        DialogFixture::press(&mut dialog, &service, KeyCode::BackTab);
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        let request = DialogFixture::submitted(step);
        assert_eq!(request.prompt(), "fix");
        assert_eq!(request.mode(), AgentMode::Build);
    }

    #[test]
    fn a_new_session_starts_in_plan_mode_when_the_agent_has_plan_args() {
        let (dialog, _service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        assert!(
            DialogFixture::text(&dialog)
                .contains("Agent     claude\nCheckout  new worktree\nMode      plan\n")
        );
    }

    #[test]
    fn shift_tab_shows_build_mode() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        DialogFixture::press(&mut dialog, &service, KeyCode::BackTab);

        assert!(
            DialogFixture::text(&dialog)
                .contains("Agent     claude\nCheckout  new worktree\nMode      build\n")
        );
    }

    #[test]
    fn a_second_shift_tab_goes_back_to_plan_mode() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        DialogFixture::press(&mut dialog, &service, KeyCode::BackTab);
        DialogFixture::press(&mut dialog, &service, KeyCode::BackTab);

        assert!(DialogFixture::text(&dialog).contains("Mode      plan\n"));
    }

    #[test]
    fn shift_tab_keeps_the_focused_field() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        DialogFixture::press(&mut dialog, &service, KeyCode::BackTab);
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");

        assert!(DialogFixture::text(&dialog).starts_with("Name      fix-auth"));
    }

    #[test]
    fn a_new_session_starts_in_a_new_worktree() {
        let (dialog, _service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        assert!(DialogFixture::text(&dialog).contains("Checkout  new worktree\n"));
    }

    #[test]
    fn ctrl_t_switches_to_the_main_checkout() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        DialogFixture::control(&mut dialog, &service, 't');

        assert!(DialogFixture::text(&dialog).contains("Checkout  main checkout\n"));
    }

    #[test]
    fn ctrl_t_twice_switches_back_to_a_new_worktree() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        DialogFixture::control(&mut dialog, &service, 't');
        DialogFixture::control(&mut dialog, &service, 't');

        assert!(DialogFixture::text(&dialog).contains("Checkout  new worktree\n"));
    }

    #[test]
    fn ctrl_t_keeps_the_focus_and_types_nothing() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        DialogFixture::type_text(&mut dialog, &service, "fix");

        DialogFixture::control(&mut dialog, &service, 't');

        assert!(DialogFixture::text(&dialog).starts_with("Name      fix█\n"));
        assert_eq!(DialogFixture::hints(&dialog), NewSessionField::Name.hints());
    }

    #[test]
    fn ctrl_t_switches_the_checkout_while_the_repo_search_is_open() {
        let (mut dialog, service) =
            DialogFixture::opened(&DialogFixture::launched_among_gateways());
        DialogFixture::search(&mut dialog, &service, "gate");

        DialogFixture::control(&mut dialog, &service, 't');

        let shown = DialogFixture::text(&dialog);
        assert!(shown.contains("Find: gate"));
        assert!(shown.contains("Checkout  main checkout\n"));
    }

    #[test]
    fn create_after_ctrl_t_asks_for_the_main_checkout() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());

        DialogFixture::control(&mut dialog, &service, 't');
        let step = DialogFixture::create(&mut dialog, &service);

        assert_eq!(
            DialogFixture::submitted(step),
            CreateRequest::new("fix-auth", ServiceFixture::repo())
                .with_agent("claude")
                .with_checkout(Checkout::Main)
                .with_mode(AgentMode::Plan)
        );
    }

    #[test]
    fn the_main_checkout_is_drawn_in_the_warning_color() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        DialogFixture::control(&mut dialog, &service, 't');

        assert_eq!(DialogFixture::in_warning_color(&dialog), "main checkout");
    }

    #[test]
    fn tab_never_stops_on_the_agent_checkout_or_mode_rows() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);
        let on_the_repo = DialogFixture::hints(&dialog);

        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);

        assert_eq!(on_the_repo, NewSessionField::Repo.hints());
        assert_eq!(DialogFixture::hints(&dialog), DialogFixture::PROMPT_HINTS);
    }

    #[test]
    fn shift_tab_switches_the_mode_while_the_repo_search_is_open() {
        let (mut dialog, service) =
            DialogFixture::opened(&DialogFixture::launched_among_gateways());
        DialogFixture::search(&mut dialog, &service, "gate");

        DialogFixture::press(&mut dialog, &service, KeyCode::BackTab);

        let shown = DialogFixture::text(&dialog);
        assert!(shown.contains("Find: gate"));
        assert!(shown.contains("Mode      build\n"));
    }

    #[test]
    fn create_after_shift_tab_asks_for_build_mode() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());

        DialogFixture::press(&mut dialog, &service, KeyCode::BackTab);
        let step = DialogFixture::create(&mut dialog, &service);

        assert_eq!(
            DialogFixture::submitted(step),
            CreateRequest::new("fix-auth", ServiceFixture::repo()).with_agent("claude")
        );
    }

    #[test]
    fn plan_mode_warns_before_create_when_the_agent_has_no_plan_args() {
        let agent = Agent::new("example-agent", "sh");
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(vec![agent]));
        let (mut dialog, service) = DialogFixture::opened(&fixture);

        DialogFixture::press(&mut dialog, &service, KeyCode::BackTab);

        assert!(
            DialogFixture::text(&dialog).contains("Mode      plan · this agent has no plan_args\n")
        );
    }

    #[test]
    fn a_new_session_starts_in_build_mode_when_the_agent_has_no_plan_args() {
        let agent = Agent::new("example-agent", "sh");
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(vec![agent]));
        let (dialog, _service) = DialogFixture::opened(&fixture);

        assert!(DialogFixture::text(&dialog).contains("Mode      build\n"));
    }

    #[test]
    fn alt_p_to_an_agent_without_plan_args_keeps_plan_mode_and_warns() {
        let agents = vec![ServiceFixture::agent(), Agent::new("example-agent", "sh")];
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(agents));
        let (mut dialog, service) = DialogFixture::opened(&fixture);

        DialogFixture::cycle_agent(&mut dialog, &service);

        assert!(
            DialogFixture::text(&dialog).contains("Mode      plan · this agent has no plan_args\n")
        );
    }

    #[test]
    fn plan_mode_for_an_agent_without_plan_args_is_refused() {
        let agent = Agent::new("example-agent", "sh");
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(vec![agent]));
        let (mut dialog, service) = DialogFixture::focused_on_prompt(&fixture);

        DialogFixture::press(&mut dialog, &service, KeyCode::BackTab);
        let step = DialogFixture::create(&mut dialog, &service);

        assert_eq!(step, DialogStep::Stay);
        assert!(
            DialogFixture::flat_text(&dialog)
                .contains("agent example-agent cannot start in plan mode, it has no plan_args")
        );
    }

    #[test]
    fn ctrl_s_creates_from_the_prompt() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        DialogFixture::type_text(&mut dialog, &service, "fix");

        let step = DialogFixture::create(&mut dialog, &service);

        assert_eq!(DialogFixture::submitted(step).prompt(), "fix");
    }

    #[test]
    fn esc_in_the_prompt_cancels_the_dialog() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        DialogFixture::type_text(&mut dialog, &service, "fix");

        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Esc);

        assert_eq!(step, DialogStep::Close);
    }

    #[test]
    fn the_prompt_shows_no_cursor_until_it_has_focus() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        DialogFixture::type_text(&mut dialog, &service, "fix");
        let focused = DialogFixture::shows_a_cursor(&dialog);

        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);

        assert!(focused);
        assert!(!DialogFixture::shows_a_cursor(&dialog));
    }

    #[test]
    fn a_selection_in_the_prompt_is_underlined_apart_from_the_cursor() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        DialogFixture::type_text(&mut dialog, &service, "fix the tests");
        for _ in 0.."the tests".len() {
            DialogFixture::press(&mut dialog, &service, KeyCode::Left);
        }
        let select_right = KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT);

        for _ in 0.."the".len() {
            dialog.handle_key(&select_right, &service);
        }

        assert_eq!(DialogFixture::underlined(&dialog), "the");
        assert!(DialogFixture::text_with_cursor(&dialog).contains("Prompt    fix the█tests\n"));
    }

    #[test]
    fn a_selection_in_the_prompt_hides_with_the_cursor() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        DialogFixture::type_text(&mut dialog, &service, "fix the tests");
        let select_left = KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT);
        for _ in 0.."tests".len() {
            dialog.handle_key(&select_left, &service);
        }
        let focused = DialogFixture::underlined(&dialog);

        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);

        assert_eq!(focused, "tests");
        assert_eq!(DialogFixture::underlined(&dialog), "");
        assert!(!DialogFixture::shows_a_cursor(&dialog));
    }

    #[test]
    fn an_empty_prompt_takes_one_row() {
        let (dialog, _service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());

        assert!(
            DialogFixture::text_with_cursor(&dialog).contains(
                "Agent     claude\nCheckout  new worktree\nMode      plan\nPrompt    █\n\n"
            )
        );
    }

    #[test]
    fn a_tall_prompt_shows_six_rows_ending_at_the_cursor() {
        let (mut dialog, _service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());

        dialog.paste(DialogFixture::SEVEN_LINES);

        assert!(DialogFixture::text_with_cursor(&dialog).contains(
            "Agent     claude\nCheckout  new worktree\nMode      plan\nPrompt    two\n          three\n          four\n          five\n          six\n          seven█\n\n"
        ));
    }

    #[test]
    fn moving_up_a_tall_prompt_scrolls_back_to_its_first_rows() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        dialog.paste(DialogFixture::SEVEN_LINES);
        let scrolled = DialogFixture::text_with_cursor(&dialog);

        for _ in 0..6 {
            DialogFixture::press(&mut dialog, &service, KeyCode::Up);
        }

        assert!(
            scrolled.contains(
                "Agent     claude\nCheckout  new worktree\nMode      plan\nPrompt    two\n"
            )
        );
        assert!(DialogFixture::text_with_cursor(&dialog).contains(
            "Prompt    one█\n          two\n          three\n          four\n          five\n          six\n\n"
        ));
    }

    #[test]
    fn deleting_the_last_line_of_a_scrolled_prompt_brings_back_its_first_line() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        dialog.paste(DialogFixture::SEVEN_LINES);
        let scrolled = DialogFixture::text_with_cursor(&dialog);

        for _ in 0.."\nseven".len() {
            DialogFixture::press(&mut dialog, &service, KeyCode::Backspace);
        }

        assert!(
            scrolled.contains(
                "Agent     claude\nCheckout  new worktree\nMode      plan\nPrompt    two\n"
            )
        );
        assert!(DialogFixture::text_with_cursor(&dialog).contains(
            "Prompt    one\n          two\n          three\n          four\n          five\n          six█\n\n"
        ));
    }

    #[test]
    fn bringing_back_the_first_line_leaves_the_cursor_at_the_end_of_a_full_row() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        let text_width = usize::from(DialogFixture::WIDTH) - DialogLabels::full().width() - 1;
        let full_row = "x".repeat(text_width);
        dialog.paste(&format!("one\ntwo\nthree\nfour\nfive\n{full_row}\nseven"));
        let scrolled = DialogFixture::text_with_cursor(&dialog);
        for _ in 0.."\nseven".len() {
            DialogFixture::press(&mut dialog, &service, KeyCode::Backspace);
        }
        let restored = DialogFixture::text_with_cursor(&dialog);

        DialogFixture::type_text(&mut dialog, &service, "!");
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert!(scrolled.contains("Prompt    two\n"));
        assert!(restored.contains("Prompt    one\n"));
        assert!(
            DialogFixture::submitted(step)
                .prompt()
                .ends_with(&format!("{full_row}!"))
        );
    }

    #[test]
    fn bringing_back_the_first_line_keeps_the_cursor_after_an_emoji_ending_a_line() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        let heart_line = "love \u{2764}\u{fe0f}";
        dialog.paste(&format!("one\ntwo\nthree\nfour\nfive\n{heart_line}\nseven"));
        let scrolled = DialogFixture::text(&dialog);
        for _ in 0.."\nseven".len() {
            DialogFixture::press(&mut dialog, &service, KeyCode::Backspace);
        }
        let restored = DialogFixture::text(&dialog);

        DialogFixture::type_text(&mut dialog, &service, "!");
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert!(scrolled.contains("Prompt    two\n"));
        assert!(restored.contains("Prompt    one\n"));
        assert!(
            DialogFixture::submitted(step)
                .prompt()
                .ends_with(&format!("\n{heart_line}!"))
        );
    }

    #[test]
    fn bringing_back_the_first_line_keeps_the_cursor_after_combining_marks_mid_line() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        let accented = "cafe\u{301}\u{302}";
        dialog.paste(&format!("one\ntwo\nthree\nfour\nfive\n{accented}\nseven"));
        let scrolled = DialogFixture::text(&dialog);
        DialogFixture::press(&mut dialog, &service, KeyCode::Home);
        DialogFixture::press(&mut dialog, &service, KeyCode::Backspace);
        let restored = DialogFixture::text(&dialog);

        DialogFixture::type_text(&mut dialog, &service, "!");
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert!(scrolled.contains("Prompt    two\n"));
        assert!(restored.contains("Prompt    one\n"));
        assert!(
            DialogFixture::submitted(step)
                .prompt()
                .ends_with(&format!("\n{accented}!seven"))
        );
    }

    #[test]
    fn typing_after_a_pasted_line_longer_than_u16_lands_at_its_end() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        let long_line = "x".repeat(usize::from(u16::MAX) * 2);
        dialog.paste(&long_line);
        let drawn = DialogFixture::text(&dialog);

        DialogFixture::type_text(&mut dialog, &service, "Z");
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert!(drawn.contains("Prompt    xxx"));
        assert_eq!(
            DialogFixture::submitted(step).prompt(),
            format!("{long_line}Z")
        );
    }

    #[test]
    fn a_prompt_given_back_its_rows_shows_its_first_lines_again() {
        let (mut dialog, _service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        let one_prompt_row = 6;
        dialog.paste("one\ntwo\nthree");
        let squeezed = DialogFixture::text_at(&dialog, one_prompt_row);

        let roomy = DialogFixture::text_with_cursor(&dialog);

        assert!(squeezed.contains("Prompt    three\n"));
        assert!(roomy.contains("Prompt    one\n          two\n          three█\n"));
    }

    #[test]
    fn typing_on_an_earlier_line_edits_it_in_place() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        dialog.paste("fix the test\nthen push");

        DialogFixture::press(&mut dialog, &service, KeyCode::Up);
        DialogFixture::press(&mut dialog, &service, KeyCode::End);
        DialogFixture::type_text(&mut dialog, &service, "s");
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(
            DialogFixture::submitted(step).prompt(),
            "fix the tests\nthen push"
        );
    }

    #[test]
    fn a_dialog_with_no_room_for_the_prompt_still_shows_its_cursor_row() {
        let (mut dialog, _service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        let no_spare_rows = 5;

        dialog.paste("one\ntwo\nthree");

        let shown = DialogFixture::text_at(&dialog, no_spare_rows);
        assert!(shown.contains("Prompt    three\n"));
        assert!(shown.ends_with(DialogFixture::PROMPT_HINTS));
    }

    #[test]
    fn the_hints_keep_their_row_when_an_error_appears() {
        let (mut dialog, _service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        let rows_without_an_error = DialogFixture::text(&dialog).lines().count();

        dialog.fail("session fix-auth already exists".to_string());

        assert_eq!(
            DialogFixture::text(&dialog).lines().count(),
            rows_without_an_error
        );
    }

    #[test]
    fn a_short_dialog_keeps_the_chosen_agent_above_the_checkout_row() {
        let (mut dialog, service) = DialogFixture::opened_with_agents(5);

        for _ in 0..4 {
            DialogFixture::cycle_agent(&mut dialog, &service);
        }

        let shown = DialogFixture::text_at(&dialog, DialogFixture::COMPACT_HEIGHT);
        assert!(shown.contains(
            "Agent     agent-5 · no prompt\nCheckout  new worktree\nMode      build\nPrompt"
        ));
        assert!(shown.ends_with(NewSessionField::Name.hints()));
    }

    #[test]
    fn a_dialog_too_short_for_the_search_keeps_its_first_match_in_view() {
        let (mut dialog, service) =
            DialogFixture::opened(&DialogFixture::launched_among_gateways());
        DialogFixture::search(&mut dialog, &service, "gate");
        DialogFixture::settle(&mut dialog, |text| text.contains("20/20 repos under ~"));
        let rows_without_the_name = 9;

        dialog.fail(DialogFixture::worktree_error(1));

        let shown = DialogFixture::text_at(&dialog, rows_without_the_name);
        assert!(shown.starts_with("Repo      Find: gate█\n"));
        assert!(shown.contains(
            "~/work/gateway-01\nAgent     claude\nCheckout  new worktree\nMode      plan\nPrompt"
        ));
    }

    #[test]
    fn a_short_dialog_gives_the_rows_kept_for_an_error_to_a_tall_prompt() {
        let (mut dialog, _service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());

        dialog.paste(DialogFixture::SEVEN_LINES);

        let shown = DialogFixture::text_at(&dialog, DialogFixture::COMPACT_HEIGHT);
        assert!(shown.contains("Prompt    five\n          six\n          seven\n\n"));
        assert!(shown.ends_with(DialogFixture::PROMPT_HINTS));
    }

    #[test]
    fn a_short_dialog_without_an_error_keeps_every_field() {
        let (dialog, _service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        let one_prompt_row = 7;

        let shown = DialogFixture::text_at(&dialog, one_prompt_row);

        assert_eq!(
            shown,
            format!(
                "Name      fix-auth\nRepo      example-project\nAgent     claude\nCheckout  new worktree\nMode      plan\nPrompt\n{}",
                DialogFixture::PROMPT_HINTS
            )
        );
    }

    #[test]
    fn a_repo_search_gives_its_spare_rows_to_a_tall_prompt() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_among_gateways());
        dialog.paste(DialogFixture::SEVEN_LINES);
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);

        DialogFixture::search(&mut dialog, &service, "gate");
        DialogFixture::settle(&mut dialog, |text| text.contains("20/20 repos under ~"));

        let shown = DialogFixture::text(&dialog);
        assert!(shown.contains(
            "gateway-05\nAgent     claude\nCheckout  new worktree\nMode      plan\nPrompt    two\n"
        ));
        assert!(shown.ends_with(DialogFixture::SEARCH_HINTS));
    }

    #[test]
    fn a_short_dialog_keeps_the_hints_in_view_with_the_repo_search_open() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_among_gateways());
        dialog.paste(DialogFixture::SEVEN_LINES);
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);

        DialogFixture::search(&mut dialog, &service, "gate");
        DialogFixture::settle(&mut dialog, |text| text.contains("20/20 repos under ~"));

        let shown = DialogFixture::text_at(&dialog, DialogFixture::COMPACT_HEIGHT);
        assert!(shown.contains("gateway-01\nAgent     claude\nCheckout  new worktree\nMode      plan\nPrompt    seven\n"));
        assert!(shown.ends_with(DialogFixture::SEARCH_HINTS));
    }

    #[test]
    fn a_wrapped_error_in_a_short_dialog_takes_its_rows_from_the_prompt() {
        let (mut dialog, _service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        dialog.paste(DialogFixture::SEVEN_LINES);
        let error = DialogFixture::worktree_error(5);

        dialog.fail(error.clone());

        let shown = DialogFixture::text_at(&dialog, DialogFixture::COMPACT_HEIGHT);
        assert!(shown.starts_with(
            "Name      fix-auth\nRepo      example-project\nAgent     claude\nCheckout  new worktree\nMode      plan\nPrompt    seven\n"
        ));
        assert!(shown.replace('\n', " ").contains(&error));
        assert!(shown.ends_with(DialogFixture::PROMPT_HINTS));
    }

    #[test]
    fn a_dialog_too_short_for_every_field_keeps_the_mode_above_the_prompt() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        let rows_without_the_repo = 6;

        DialogFixture::press(&mut dialog, &service, KeyCode::BackTab);

        let shown = DialogFixture::text_at(&dialog, rows_without_the_repo);
        assert!(shown.contains("Mode      build\nPrompt"));
    }

    #[test]
    fn a_dialog_too_short_for_every_field_keeps_the_agent_above_the_checkout() {
        let (dialog, _service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        let rows_without_the_repo = 6;

        let shown = DialogFixture::text_at(&dialog, rows_without_the_repo);

        assert!(shown.contains("Agent     claude\nCheckout  new worktree\n"));
    }

    #[test]
    fn a_dialog_too_short_for_every_field_keeps_the_prompt_the_error_and_the_hints() {
        let (mut dialog, _service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        dialog.paste(DialogFixture::SEVEN_LINES);
        let error = DialogFixture::worktree_error(3);
        let rows_for_all_but_the_repo = 8;

        dialog.fail(error.clone());

        let shown = DialogFixture::text_at(&dialog, rows_for_all_but_the_repo);
        assert!(shown.starts_with(
            "Name      fix-auth\nAgent     claude\nCheckout  new worktree\nMode      plan\nPrompt    seven\n"
        ));
        assert!(shown.replace('\n', " ").contains(&error));
        assert!(shown.ends_with(DialogFixture::PROMPT_HINTS));
    }

    #[test]
    fn a_prompt_for_an_agent_without_the_placeholder_is_refused() {
        let agent = Agent::new("plain-agent", "plain-agent");
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(vec![agent]));
        let (mut dialog, service) = DialogFixture::focused_on_prompt(&fixture);
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
    fn alt_p_marks_an_agent_without_a_prompt() {
        let agents = vec![
            ServiceFixture::agent(),
            Agent::new("plain-agent", "plain-agent"),
        ];
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(agents));
        let (mut dialog, service) = DialogFixture::opened(&fixture);

        DialogFixture::cycle_agent(&mut dialog, &service);

        assert!(
            DialogFixture::text(&dialog).contains("Agent     plain-agent · no prompt\nCheckout")
        );
    }

    #[test]
    fn a_failed_create_shows_its_error_in_the_dialog() {
        let fixture = DialogFixture::launched_in_repo();
        let (mut dialog, _service) = DialogFixture::opened(&fixture);

        dialog.fail("session fix-auth already exists".to_string());

        assert!(DialogFixture::text(&dialog).contains("session fix-auth already exists"));
    }
}

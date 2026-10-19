use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;

use super::agent_picker::AgentPicker;
use super::new_session_field::NewSessionField;
use super::new_session_view::NewSessionView;
use super::prompt_field::PromptField;
use super::repo_picker::{PickerStep, RepoPicker};
use super::setting::Setting;
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
    const NAME_MISSING: &'static str = "enter a session name";
    const REPO_MISSING: &'static str = "pick a repo";

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
        let step = self.repos.poll();
        self.follow_picker(step);
    }

    pub fn fail(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn handle_key(&mut self, key: &KeyEvent, service: &SessionService) -> DialogStep {
        if let Some(setting) = Setting::for_key(key) {
            self.change(setting);
            return DialogStep::Stay;
        }
        if self.focus == NewSessionField::Repo {
            let step = self.repos.handle_key(key, service);
            self.follow_picker(step);
            if step != PickerStep::Ignored {
                return DialogStep::Stay;
            }
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

    fn follow_picker(&mut self, step: PickerStep) {
        match step {
            PickerStep::Ignored | PickerStep::Handled => {}
            PickerStep::Edited => self.clear_error(),
            PickerStep::Picked => self.move_past_repo(),
        }
    }

    fn move_past_repo(&mut self) {
        self.focus = NewSessionField::Repo.next();
    }

    fn change(&mut self, setting: Setting) {
        match setting {
            Setting::Mode => self.mode = self.mode.toggled(),
            Setting::Agent => self.agents.cycle(),
            Setting::Checkout => self.checkout = self.checkout.toggled(),
        }
    }

    fn creates(key: &KeyEvent) -> bool {
        key.code == KeyCode::Char(Self::CREATE_KEY) && key.modifiers == KeyModifiers::CONTROL
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
        let edited = match self.focus {
            NewSessionField::Name => self.name.edit(key),
            NewSessionField::Prompt => self.prompt.edit(key),
            NewSessionField::Repo => false,
        };
        if edited {
            self.clear_error();
        }
    }

    fn clear_error(&mut self) {
        self.error = None;
    }

    fn submit(&mut self) -> DialogStep {
        match self.request() {
            Ok(request) => {
                DialogStep::Request(AppRequest::Block(BlockingOperation::Create(request)))
            }
            Err(refusal) => {
                self.refuse(refusal);
                DialogStep::Stay
            }
        }
    }

    fn refuse(&mut self, refusal: Refusal) {
        match refusal {
            Refusal::MissingField(field, problem) => {
                self.focus = field;
                self.error = Some(problem.to_string());
            }
            Refusal::General(problem) => self.error = Some(problem),
        }
    }

    fn request(&self) -> Result<CreateRequest, Refusal> {
        let name = self.name.value().trim();
        if name.is_empty() {
            return Err(Refusal::MissingField(
                NewSessionField::Name,
                Self::NAME_MISSING,
            ));
        }
        let repo = self.repos.selected_repo().ok_or(Refusal::MissingField(
            NewSessionField::Repo,
            Self::REPO_MISSING,
        ))?;
        let agent = self.agents.selected().map_err(Refusal::General)?;
        let prompt = self.prompt.value();
        if let Some(problem) = agent.launch_refusal(self.mode, &prompt) {
            return Err(Refusal::General(problem));
        }
        Ok(CreateRequest::new(name, repo.clone())
            .with_agent(agent.name())
            .with_checkout(self.checkout)
            .with_mode(self.mode)
            .with_prompt(&prompt))
    }
}

enum Refusal {
    MissingField(NewSessionField, &'static str),
    General(String),
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
        const WIDTH: u16 = 72;
        const FULL_HEIGHT: u16 = 20;
        const SIX_ROW_HEIGHT: u16 = 14;
        const COMPACT_HEIGHT: u16 = 10;
        const CURSOR: &'static str = "█";

        const FOUND: &'static str = "/home/example/work/api-gateway";
        const WORK: &'static str = "/home/example/work";
        const GATEWAY_COUNT: usize = 20;
        const TYPED: &'static str = "/srv/tools";
        const NAME_HINTS: &'static str = "Enter next field · Ctrl-S create · Esc cancel";
        const REPO_HINTS: &'static str =
            "Type to find a repo · Enter next field · Ctrl-S create · Esc cancel";
        const PROMPT_HINTS: &'static str = "Enter create · Esc cancel";
        const SEARCH_HINTS: &'static str =
            "Type to search · ↑ ↓ Ctrl-J Ctrl-K choose · Enter pick · Esc back";
        const DEFAULT_SETTINGS: &'static str =
            "  plan mode (Alt-j) · claude (Alt-k) · new worktree (Alt-l)";
        const RULE: &'static str = "─";
        const EMPTY_PROMPT_ROW: &'static str = "❯";
        const TEXT_INDENT: usize = 2;
        const CURSOR_COLUMN: usize = 1;
        const PANEL_WIDTH: usize = Self::WIDTH as usize;
        const PROMPT_TEXT_WIDTH: usize =
            Self::PANEL_WIDTH - Self::TEXT_INDENT - Self::CURSOR_COLUMN;
        const COMPACT_TIER_WIDTH: u16 = 54;
        const COMPACT_TIER_HEIGHT: u16 = 8;
        const COMPACT_PANEL_WIDTH: usize = Self::COMPACT_TIER_WIDTH as usize;
        const SEVEN_LINES: &'static str = "one\ntwo\nthree\nfour\nfive\nsix\nseven";
        const CREATE_FAILURE: &'static str = "session fix-auth already exists";
        const PROMPT_REFUSAL: &'static str =
            "agent plain-agent cannot take a prompt, its prompt_args has no {prompt}";
        const TWO_LINE_PATH: &'static str = "/work\n/other";
        const LINE_BREAK_IN_PATH: &'static str = "a path cannot contain a line break";

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

        fn refused_for_a_missing_name() -> (NewSessionDialog, SessionService) {
            let (mut dialog, service) = Self::opened(&Self::launched_in_repo());
            Self::create(&mut dialog, &service);
            (dialog, service)
        }

        fn refused_on_the_prompt() -> (NewSessionDialog, SessionService) {
            let agent = Agent::new("plain-agent", "plain-agent");
            let fixture = Self::launched_in_repo()
                .with_config(ServiceFixture::config().with_agents(vec![agent]));
            let (mut dialog, service) = Self::focused_on_prompt(&fixture);
            Self::type_text(&mut dialog, &service, "fix the tests");
            Self::press(&mut dialog, &service, KeyCode::Enter);
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
            Self::text_with_cursor_at(dialog, Self::FULL_HEIGHT)
        }

        fn text_with_cursor_at(dialog: &NewSessionDialog, height: u16) -> String {
            let screen = Self::screen(dialog, height);
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
            Self::screen_sized(dialog, Self::WIDTH, height, DialogLabels::full())
        }

        fn screen_sized(
            dialog: &NewSessionDialog,
            width: u16,
            height: u16,
            labels: DialogLabels,
        ) -> Buffer {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| dialog.render(frame, frame.area(), labels, &Theme))
                .unwrap();
            terminal.backend().buffer().clone()
        }

        fn compact_text(dialog: &NewSessionDialog) -> String {
            let screen = Self::screen_sized(
                dialog,
                Self::COMPACT_TIER_WIDTH,
                Self::COMPACT_TIER_HEIGHT,
                DialogLabels::abbreviated(),
            );
            Self::rows(&screen, |cell| cell.symbol())
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

        fn prompt_text(dialog: &NewSessionDialog) -> String {
            Self::prompt_in(&Self::text(dialog))
        }

        fn prompt_text_with_cursor(dialog: &NewSessionDialog) -> String {
            Self::prompt_in(&Self::text_with_cursor(dialog))
        }

        fn prompt_in(text: &str) -> String {
            text.lines()
                .skip_while(|row| !Self::is_rule(row))
                .skip(1)
                .take_while(|row| !Self::is_rule(row))
                .map(Self::without_indent)
                .collect::<Vec<_>>()
                .join("\n")
        }

        fn is_rule(row: &str) -> bool {
            row.starts_with(Self::RULE)
        }

        fn without_indent(row: &str) -> String {
            row.chars().skip(Self::TEXT_INDENT).collect()
        }

        fn rule(width: usize) -> String {
            Self::RULE.repeat(width)
        }

        fn rules_are_dim(dialog: &NewSessionDialog) -> bool {
            let screen = Self::screen(dialog, Self::FULL_HEIGHT);
            let rule = screen
                .content()
                .iter()
                .find(|cell| cell.symbol() == Self::RULE);
            rule.is_some_and(|cell| cell.modifier.contains(Modifier::DIM))
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

        fn toggle_mode(dialog: &mut NewSessionDialog, service: &SessionService) -> DialogStep {
            Self::chord(dialog, service, KeyModifiers::ALT, 'j')
        }

        fn cycle_agent(dialog: &mut NewSessionDialog, service: &SessionService) -> DialogStep {
            Self::chord(dialog, service, KeyModifiers::ALT, 'k')
        }

        fn toggle_checkout(dialog: &mut NewSessionDialog, service: &SessionService) -> DialogStep {
            Self::chord(dialog, service, KeyModifiers::ALT, 'l')
        }

        fn change_every_setting(dialog: &mut NewSessionDialog, service: &SessionService) {
            Self::toggle_mode(dialog, service);
            Self::cycle_agent(dialog, service);
            Self::toggle_checkout(dialog, service);
        }

        fn twelve_lines() -> String {
            (1..=12)
                .map(|number| format!("line {number}"))
                .collect::<Vec<_>>()
                .join("\n")
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

        fn shown_error(dialog: &NewSessionDialog) -> String {
            let shown = Self::text(dialog);
            let row_above_the_hints = shown.lines().rev().nth(1);
            row_above_the_hints.unwrap_or_default().to_string()
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
                DialogFixture::NAME_HINTS,
                DialogFixture::REPO_HINTS,
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

        assert!(on_the_field.ends_with(DialogFixture::REPO_HINTS));
        assert!(DialogFixture::text(&dialog).ends_with(DialogFixture::SEARCH_HINTS));
    }

    #[test]
    fn a_pasted_search_with_an_inner_line_break_is_a_field_error() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        DialogFixture::search(&mut dialog, &service, "g");

        dialog.paste(DialogFixture::TWO_LINE_PATH);

        assert!(DialogFixture::text(&dialog).contains(DialogFixture::LINE_BREAK_IN_PATH));
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
    fn alt_k_picks_the_next_agent() {
        let (mut dialog, service) =
            DialogFixture::opened(&DialogFixture::launched_with_two_agents());
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");

        DialogFixture::cycle_agent(&mut dialog, &service);
        let step = DialogFixture::create(&mut dialog, &service);

        assert_eq!(DialogFixture::submitted(step).agent(), Some("opencode"));
    }

    #[test]
    fn alt_k_past_the_last_agent_wraps_to_the_first() {
        let (mut dialog, service) =
            DialogFixture::opened(&DialogFixture::launched_with_two_agents());
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");

        DialogFixture::cycle_agent(&mut dialog, &service);
        DialogFixture::cycle_agent(&mut dialog, &service);
        let step = DialogFixture::create(&mut dialog, &service);

        assert_eq!(DialogFixture::submitted(step).agent(), Some("claude"));
    }

    #[test]
    fn alt_k_shows_the_next_agent_in_the_settings_line() {
        let (mut dialog, service) =
            DialogFixture::opened(&DialogFixture::launched_with_two_agents());

        DialogFixture::cycle_agent(&mut dialog, &service);

        assert!(
            DialogFixture::text(&dialog)
                .contains("\n  plan mode (Alt-j) · opencode (Alt-k) · new worktree (Alt-l)\n")
        );
    }

    #[test]
    fn alt_k_keeps_the_dialog_open_on_the_focused_field_and_types_nothing() {
        let (mut dialog, service) =
            DialogFixture::opened(&DialogFixture::launched_with_two_agents());
        DialogFixture::type_text(&mut dialog, &service, "ship");

        let step = DialogFixture::cycle_agent(&mut dialog, &service);

        assert_eq!(step, DialogStep::Stay);
        assert!(DialogFixture::text(&dialog).starts_with("Name      ship█\n"));
        assert_eq!(DialogFixture::hints(&dialog), NewSessionField::Name.hints());
    }

    #[test]
    fn alt_k_switches_the_agent_without_typing_into_the_prompt() {
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
    fn alt_k_switches_the_agent_while_the_repo_search_is_open() {
        let (mut dialog, service) =
            DialogFixture::opened(&DialogFixture::launched_with_two_agents());
        DialogFixture::search(&mut dialog, &service, "gate");

        DialogFixture::cycle_agent(&mut dialog, &service);

        let shown = DialogFixture::text(&dialog);
        assert!(shown.contains("Find: gate█"));
        assert!(shown.contains("· opencode (Alt-k) ·"));
    }

    #[test]
    fn alt_k_with_no_agents_keeps_the_config_path_error() {
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(Vec::new()));
        let (mut dialog, service) = DialogFixture::opened(&fixture);

        DialogFixture::cycle_agent(&mut dialog, &service);

        assert!(
            DialogFixture::text(&dialog)
                .contains("\n  no agents configured in /config/ringleader/config.toml (Alt-k)\n")
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

        assert_eq!(
            DialogFixture::prompt_text_with_cursor(&dialog),
            "one\ntwo\nthree█"
        );
    }

    #[test]
    fn a_pasted_prompt_keeps_the_indent_of_each_line() {
        let (mut dialog, _service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());

        dialog.paste("steps:\n  - run: cargo test");

        assert_eq!(
            DialogFixture::prompt_text(&dialog),
            "steps:\n  - run: cargo test"
        );
    }

    #[test]
    fn a_long_typed_prompt_wraps_and_submits_as_one_line() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        let prompt = "fix the flaky login test that times out on the first run after a cold start";

        DialogFixture::type_text(&mut dialog, &service, prompt);
        let shown = DialogFixture::text_with_cursor(&dialog);
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(
            DialogFixture::prompt_in(&shown),
            "fix the flaky login test that times out on the first run after a cold\n start█"
        );
        assert_eq!(DialogFixture::submitted(step).prompt(), prompt);
    }

    #[test]
    fn the_cursor_stays_in_view_as_the_prompt_reaches_the_right_edge() {
        let text_width = DialogFixture::PROMPT_TEXT_WIDTH;

        let shown = [text_width, text_width + 1].map(|length| {
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

        assert_eq!(DialogFixture::hints(&dialog), DialogFixture::NAME_HINTS);
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
                .contains(&format!("\n{}\n", DialogFixture::DEFAULT_SETTINGS))
        );
    }

    #[test]
    fn alt_j_shows_build_mode() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        DialogFixture::toggle_mode(&mut dialog, &service);

        assert!(
            DialogFixture::text(&dialog)
                .contains("\n  build mode (Alt-j) · claude (Alt-k) · new worktree (Alt-l)\n")
        );
    }

    #[test]
    fn shift_tab_still_toggles_the_mode() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        DialogFixture::press(&mut dialog, &service, KeyCode::BackTab);

        assert!(DialogFixture::text(&dialog).contains("build mode (Alt-j)"));
    }

    #[test]
    fn a_second_alt_j_goes_back_to_plan_mode() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        DialogFixture::toggle_mode(&mut dialog, &service);
        DialogFixture::toggle_mode(&mut dialog, &service);

        assert!(DialogFixture::text(&dialog).contains("plan mode (Alt-j)"));
    }

    #[test]
    fn alt_j_keeps_the_focused_field() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        DialogFixture::toggle_mode(&mut dialog, &service);
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");

        assert!(DialogFixture::text(&dialog).starts_with("Name      fix-auth"));
    }

    #[test]
    fn a_new_session_starts_in_a_new_worktree() {
        let (dialog, _service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        assert!(DialogFixture::text(&dialog).contains("new worktree (Alt-l)"));
    }

    #[test]
    fn alt_l_switches_to_the_main_checkout() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        DialogFixture::toggle_checkout(&mut dialog, &service);

        assert!(DialogFixture::text(&dialog).contains("main checkout (Alt-l)"));
    }

    #[test]
    fn alt_l_twice_switches_back_to_a_new_worktree() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        DialogFixture::toggle_checkout(&mut dialog, &service);
        DialogFixture::toggle_checkout(&mut dialog, &service);

        assert!(DialogFixture::text(&dialog).contains("new worktree (Alt-l)"));
    }

    #[test]
    fn alt_l_keeps_the_focus_and_types_nothing() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        DialogFixture::type_text(&mut dialog, &service, "fix");

        DialogFixture::toggle_checkout(&mut dialog, &service);

        assert!(DialogFixture::text(&dialog).starts_with("Name      fix█\n"));
        assert_eq!(DialogFixture::hints(&dialog), NewSessionField::Name.hints());
    }

    #[test]
    fn alt_l_switches_the_checkout_while_the_repo_search_is_open() {
        let (mut dialog, service) =
            DialogFixture::opened(&DialogFixture::launched_among_gateways());
        DialogFixture::search(&mut dialog, &service, "gate");

        DialogFixture::toggle_checkout(&mut dialog, &service);

        let shown = DialogFixture::text(&dialog);
        assert!(shown.contains("Find: gate█"));
        assert!(shown.contains("main checkout (Alt-l)"));
    }

    #[test]
    fn create_after_alt_l_asks_for_the_main_checkout() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());

        DialogFixture::toggle_checkout(&mut dialog, &service);
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

        DialogFixture::toggle_checkout(&mut dialog, &service);

        assert_eq!(DialogFixture::in_warning_color(&dialog), "main checkout");
    }

    #[test]
    fn tab_walks_only_name_repo_and_prompt() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);
        let on_the_repo = DialogFixture::hints(&dialog);

        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);

        assert_eq!(on_the_repo, NewSessionField::Repo.hints());
        assert_eq!(DialogFixture::hints(&dialog), DialogFixture::PROMPT_HINTS);
    }

    #[test]
    fn alt_j_switches_the_mode_while_the_repo_search_is_open() {
        let (mut dialog, service) =
            DialogFixture::opened(&DialogFixture::launched_among_gateways());
        DialogFixture::search(&mut dialog, &service, "gate");

        DialogFixture::toggle_mode(&mut dialog, &service);

        let shown = DialogFixture::text(&dialog);
        assert!(shown.contains("Find: gate█"));
        assert!(shown.contains("build mode (Alt-j)"));
    }

    #[test]
    fn create_after_alt_j_asks_for_build_mode() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());

        DialogFixture::toggle_mode(&mut dialog, &service);
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

        DialogFixture::toggle_mode(&mut dialog, &service);

        assert!(DialogFixture::text(&dialog).contains("plan mode, no plan_args (Alt-j)"));
    }

    #[test]
    fn a_new_session_starts_in_build_mode_when_the_agent_has_no_plan_args() {
        let agent = Agent::new("example-agent", "sh");
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(vec![agent]));
        let (dialog, _service) = DialogFixture::opened(&fixture);

        assert!(
            DialogFixture::text(&dialog)
                .contains("  build mode (Alt-j) · example-agent, no prompt (Alt-k)")
        );
    }

    #[test]
    fn alt_k_to_an_agent_without_plan_args_keeps_plan_mode_and_warns() {
        let agents = vec![ServiceFixture::agent(), Agent::new("example-agent", "sh")];
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(agents));
        let (mut dialog, service) = DialogFixture::opened(&fixture);

        DialogFixture::cycle_agent(&mut dialog, &service);

        assert!(DialogFixture::text(&dialog).contains("plan mode, no plan_args (Alt-j)"));
    }

    #[test]
    fn plan_mode_for_an_agent_without_plan_args_is_refused() {
        let agent = Agent::new("example-agent", "sh");
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(vec![agent]));
        let (mut dialog, service) = DialogFixture::focused_on_prompt(&fixture);

        DialogFixture::toggle_mode(&mut dialog, &service);
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
        assert_eq!(
            DialogFixture::prompt_text_with_cursor(&dialog),
            "fix the█tests"
        );
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

        assert_eq!(DialogFixture::prompt_text_with_cursor(&dialog), "█");
    }

    #[test]
    fn a_tall_prompt_shows_every_line_the_dialog_has_room_for() {
        let (mut dialog, _service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());

        dialog.paste(DialogFixture::SEVEN_LINES);

        assert_eq!(
            DialogFixture::prompt_text_with_cursor(&dialog),
            "one\ntwo\nthree\nfour\nfive\nsix\nseven█"
        );
    }

    #[test]
    fn a_prompt_taller_than_its_room_shows_its_last_rows_ending_at_the_cursor() {
        let (mut dialog, _service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());

        dialog.paste(DialogFixture::SEVEN_LINES);

        let shown = DialogFixture::text_with_cursor_at(&dialog, DialogFixture::SIX_ROW_HEIGHT);
        assert_eq!(
            DialogFixture::prompt_in(&shown),
            "two\nthree\nfour\nfive\nsix\nseven█"
        );
    }

    #[test]
    fn moving_up_a_tall_prompt_scrolls_back_to_its_first_rows() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        dialog.paste(DialogFixture::SEVEN_LINES);
        let scrolled = DialogFixture::text_with_cursor_at(&dialog, DialogFixture::SIX_ROW_HEIGHT);

        for _ in 0..6 {
            DialogFixture::press(&mut dialog, &service, KeyCode::Up);
        }

        let shown = DialogFixture::text_with_cursor_at(&dialog, DialogFixture::SIX_ROW_HEIGHT);
        assert!(DialogFixture::prompt_in(&scrolled).starts_with("two\n"));
        assert_eq!(
            DialogFixture::prompt_in(&shown),
            "one█\ntwo\nthree\nfour\nfive\nsix"
        );
    }

    #[test]
    fn deleting_the_last_line_of_a_scrolled_prompt_brings_back_its_first_line() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        dialog.paste(DialogFixture::SEVEN_LINES);
        let scrolled = DialogFixture::text_with_cursor_at(&dialog, DialogFixture::SIX_ROW_HEIGHT);

        for _ in 0.."\nseven".len() {
            DialogFixture::press(&mut dialog, &service, KeyCode::Backspace);
        }

        let shown = DialogFixture::text_with_cursor_at(&dialog, DialogFixture::SIX_ROW_HEIGHT);
        assert!(DialogFixture::prompt_in(&scrolled).starts_with("two\n"));
        assert_eq!(
            DialogFixture::prompt_in(&shown),
            "one\ntwo\nthree\nfour\nfive\nsix█"
        );
    }

    #[test]
    fn bringing_back_the_first_line_leaves_the_cursor_at_the_end_of_a_full_row() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        let full_row = "x".repeat(DialogFixture::PROMPT_TEXT_WIDTH);
        dialog.paste(&format!("one\ntwo\nthree\nfour\nfive\n{full_row}\nseven"));
        let scrolled = DialogFixture::text_with_cursor_at(&dialog, DialogFixture::SIX_ROW_HEIGHT);
        for _ in 0.."\nseven".len() {
            DialogFixture::press(&mut dialog, &service, KeyCode::Backspace);
        }
        let restored = DialogFixture::text_with_cursor_at(&dialog, DialogFixture::SIX_ROW_HEIGHT);

        DialogFixture::type_text(&mut dialog, &service, "!");
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert!(DialogFixture::prompt_in(&scrolled).starts_with("two\n"));
        assert!(DialogFixture::prompt_in(&restored).starts_with("one\n"));
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
        let scrolled = DialogFixture::text_at(&dialog, DialogFixture::SIX_ROW_HEIGHT);
        for _ in 0.."\nseven".len() {
            DialogFixture::press(&mut dialog, &service, KeyCode::Backspace);
        }
        let restored = DialogFixture::text_at(&dialog, DialogFixture::SIX_ROW_HEIGHT);

        DialogFixture::type_text(&mut dialog, &service, "!");
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert!(DialogFixture::prompt_in(&scrolled).starts_with("two\n"));
        assert!(DialogFixture::prompt_in(&restored).starts_with("one\n"));
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
        let scrolled = DialogFixture::text_at(&dialog, DialogFixture::SIX_ROW_HEIGHT);
        DialogFixture::press(&mut dialog, &service, KeyCode::Home);
        DialogFixture::press(&mut dialog, &service, KeyCode::Backspace);
        let restored = DialogFixture::text_at(&dialog, DialogFixture::SIX_ROW_HEIGHT);

        DialogFixture::type_text(&mut dialog, &service, "!");
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert!(DialogFixture::prompt_in(&scrolled).starts_with("two\n"));
        assert!(DialogFixture::prompt_in(&restored).starts_with("one\n"));
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

        assert!(DialogFixture::prompt_in(&drawn).starts_with("xxx"));
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

        assert_eq!(DialogFixture::prompt_in(&squeezed), "three");
        assert_eq!(DialogFixture::prompt_in(&roomy), "one\ntwo\nthree█");
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
        assert_eq!(DialogFixture::prompt_in(&shown), "three");
        assert!(shown.ends_with(DialogFixture::PROMPT_HINTS));
    }

    #[test]
    fn the_hints_keep_their_row_when_an_error_appears() {
        let (mut dialog, _service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        let rows_without_an_error = DialogFixture::text(&dialog).lines().count();

        dialog.fail(DialogFixture::CREATE_FAILURE.to_string());

        assert_eq!(
            DialogFixture::text(&dialog).lines().count(),
            rows_without_an_error
        );
    }

    #[test]
    fn a_short_dialog_keeps_the_settings_under_the_prompt() {
        let (mut dialog, service) = DialogFixture::opened_with_agents(5);

        for _ in 0..4 {
            DialogFixture::cycle_agent(&mut dialog, &service);
        }

        let shown = DialogFixture::text_at(&dialog, DialogFixture::COMPACT_HEIGHT);
        assert!(shown.contains(&format!(
            "{}\n  build mode (Alt-j) · agent-5, no prompt (Alt-k) · new worktree (Alt-l)\n",
            DialogFixture::rule(DialogFixture::PANEL_WIDTH)
        )));
        assert!(shown.ends_with(DialogFixture::NAME_HINTS));
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
        assert!(shown.contains("~/work/gateway-01\n─"));
    }

    #[test]
    fn a_short_dialog_keeps_a_row_free_for_an_error_above_a_tall_prompt() {
        let (mut dialog, _service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());

        dialog.paste(DialogFixture::SEVEN_LINES);

        let shown = DialogFixture::text_at(&dialog, DialogFixture::COMPACT_HEIGHT);
        let rows: Vec<&str> = shown.lines().collect();
        assert_eq!(DialogFixture::prompt_in(&shown), "six\nseven");
        assert_eq!(rows[rows.len() - 2], "");
        assert_eq!(rows[rows.len() - 1], DialogFixture::PROMPT_HINTS);
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
                "Name      fix-auth\nRepo      example-project\n{}\n{}\n{}\n{}\n{}",
                DialogFixture::rule(DialogFixture::PANEL_WIDTH),
                DialogFixture::EMPTY_PROMPT_ROW,
                DialogFixture::rule(DialogFixture::PANEL_WIDTH),
                DialogFixture::DEFAULT_SETTINGS,
                DialogFixture::PROMPT_HINTS
            )
        );
    }

    #[test]
    fn a_repo_search_gets_the_rows_a_tall_prompt_leaves() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_among_gateways());
        dialog.paste(DialogFixture::SEVEN_LINES);
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);

        DialogFixture::search(&mut dialog, &service, "gate");
        DialogFixture::settle(&mut dialog, |text| text.contains("20/20 repos under ~"));

        let shown = DialogFixture::text(&dialog);
        assert!(shown.contains("gateway-04\n\n─"));
        assert_eq!(DialogFixture::prompt_in(&shown), DialogFixture::SEVEN_LINES);
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
        assert!(shown.contains("gateway-01\n─"));
        assert_eq!(DialogFixture::prompt_in(&shown), "seven");
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
        assert!(shown.starts_with("Name      fix-auth\nRepo      example-project\n─"));
        assert_eq!(DialogFixture::prompt_in(&shown), "seven");
        assert!(shown.replace('\n', " ").contains(&error));
        assert!(shown.ends_with(DialogFixture::PROMPT_HINTS));
    }

    #[test]
    fn a_dialog_too_short_for_every_field_keeps_the_settings_under_the_prompt() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        let rows_without_the_repo = 6;

        DialogFixture::toggle_mode(&mut dialog, &service);

        let shown = DialogFixture::text_at(&dialog, rows_without_the_repo);
        assert!(shown.contains(&format!(
            "{}\n  build mode (Alt-j) · claude (Alt-k) · new worktree (Alt-l)\n",
            DialogFixture::rule(DialogFixture::PANEL_WIDTH)
        )));
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
        assert!(shown.starts_with("Name      fix-auth\n─"));
        assert_eq!(DialogFixture::prompt_in(&shown), "seven");
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
        assert!(DialogFixture::flat_text(&dialog).contains(DialogFixture::PROMPT_REFUSAL));
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
                .contains("\n  no agents configured in /config/ringleader/config.toml (Alt-k)\n")
        );
    }

    #[test]
    fn alt_k_marks_an_agent_without_a_prompt() {
        let agents = vec![
            ServiceFixture::agent(),
            Agent::new("plain-agent", "plain-agent"),
        ];
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(agents));
        let (mut dialog, service) = DialogFixture::opened(&fixture);

        DialogFixture::cycle_agent(&mut dialog, &service);

        assert!(DialogFixture::text(&dialog).contains("plain-agent, no prompt (Alt-k)"));
    }

    #[test]
    fn alt_j_switches_the_mode_without_typing_into_the_prompt() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        DialogFixture::type_text(&mut dialog, &service, "fix");

        DialogFixture::toggle_mode(&mut dialog, &service);
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        let request = DialogFixture::submitted(step);
        assert_eq!(request.prompt(), "fix");
        assert_eq!(request.mode(), AgentMode::Build);
    }

    #[test]
    fn the_prompt_rules_are_dim_until_the_prompt_has_focus() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        let on_the_name = DialogFixture::rules_are_dim(&dialog);

        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);

        assert!(on_the_name);
        assert!(!DialogFixture::rules_are_dim(&dialog));
    }

    #[test]
    fn the_setting_keys_type_nothing_into_the_name() {
        let (mut dialog, service) =
            DialogFixture::opened(&DialogFixture::launched_with_two_agents());
        DialogFixture::type_text(&mut dialog, &service, "fix");

        DialogFixture::change_every_setting(&mut dialog, &service);

        assert!(DialogFixture::text(&dialog).starts_with("Name      fix█\n"));
    }

    #[test]
    fn the_setting_keys_type_nothing_into_the_prompt() {
        let (mut dialog, service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_with_two_agents());
        DialogFixture::type_text(&mut dialog, &service, "fix");

        DialogFixture::change_every_setting(&mut dialog, &service);
        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(
            DialogFixture::submitted(step),
            CreateRequest::new("fix-auth", ServiceFixture::repo())
                .with_agent("opencode")
                .with_checkout(Checkout::Main)
                .with_mode(AgentMode::Build)
                .with_prompt("fix")
        );
    }

    #[test]
    fn the_setting_keys_type_nothing_into_the_repo_search() {
        let (mut dialog, service) =
            DialogFixture::opened(&DialogFixture::launched_with_two_agents());
        DialogFixture::search(&mut dialog, &service, "gate");

        DialogFixture::change_every_setting(&mut dialog, &service);

        let shown = DialogFixture::text(&dialog);
        assert!(shown.contains("Find: gate█"));
        assert!(
            shown.contains("\n  build mode (Alt-j) · opencode (Alt-k) · main checkout (Alt-l)\n")
        );
    }

    #[test]
    fn the_marks_are_drawn_in_the_warning_color() {
        let agents = vec![ServiceFixture::agent(), Agent::new("example-agent", "sh")];
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(agents));
        let (mut dialog, service) = DialogFixture::opened(&fixture);

        DialogFixture::cycle_agent(&mut dialog, &service);

        assert_eq!(
            DialogFixture::in_warning_color(&dialog),
            "no plan_argsno prompt"
        );
    }

    #[test]
    fn a_wide_settings_line_wraps_between_items() {
        let agents = vec![ServiceFixture::agent(), Agent::new("example-agent", "sh")];
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(agents));
        let (mut dialog, service) = DialogFixture::opened(&fixture);

        DialogFixture::cycle_agent(&mut dialog, &service);

        assert!(DialogFixture::text(&dialog).contains("(Alt-k)\n  new worktree (Alt-l)\n"));
    }

    #[test]
    fn the_compact_tier_abbreviates_the_labels_and_wraps_the_settings() {
        let (dialog, _service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        assert_eq!(
            DialogFixture::compact_text(&dialog),
            format!(
                "N █\nR example-project\n{}\n{}\n{}\n  plan mode (Alt-j) · claude (Alt-k)\n  new worktree (Alt-l)\n{}",
                DialogFixture::rule(DialogFixture::COMPACT_PANEL_WIDTH),
                DialogFixture::EMPTY_PROMPT_ROW,
                DialogFixture::rule(DialogFixture::COMPACT_PANEL_WIDTH),
                DialogFixture::NAME_HINTS
            )
        );
    }

    #[test]
    fn the_hints_sit_on_the_last_row_with_the_error_right_above_them() {
        let (mut dialog, _service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        dialog.fail(DialogFixture::CREATE_FAILURE.to_string());

        let rows = DialogFixture::text(&dialog).lines().count();
        assert_eq!(rows, usize::from(DialogFixture::FULL_HEIGHT));
        assert_eq!(
            DialogFixture::shown_error(&dialog),
            DialogFixture::CREATE_FAILURE
        );
        assert_eq!(DialogFixture::hints(&dialog), DialogFixture::NAME_HINTS);
    }

    #[test]
    fn a_row_above_the_hints_stays_free_for_an_error() {
        let (dialog, _service) = DialogFixture::opened(&DialogFixture::launched_in_repo());

        let rows = DialogFixture::text(&dialog).lines().count();
        assert_eq!(rows, usize::from(DialogFixture::FULL_HEIGHT));
        assert_eq!(DialogFixture::shown_error(&dialog), "");
        assert_eq!(DialogFixture::hints(&dialog), DialogFixture::NAME_HINTS);
    }

    #[test]
    fn the_prompt_keeps_its_rows_when_an_error_appears() {
        let (mut dialog, _service) =
            DialogFixture::focused_on_prompt(&DialogFixture::launched_in_repo());
        dialog.paste(&DialogFixture::twelve_lines());
        let before = DialogFixture::prompt_text(&dialog);

        dialog.fail(DialogFixture::CREATE_FAILURE.to_string());

        assert_eq!(before.lines().count(), 12);
        assert_eq!(DialogFixture::prompt_text(&dialog), before);
    }

    #[test]
    fn a_missing_name_moves_the_focus_to_the_name() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);
        DialogFixture::type_text(&mut dialog, &service, "fix");

        let step = DialogFixture::press(&mut dialog, &service, KeyCode::Enter);
        DialogFixture::type_text(&mut dialog, &service, "auth");

        assert_eq!(step, DialogStep::Stay);
        assert_eq!(DialogFixture::hints(&dialog), DialogFixture::NAME_HINTS);
        assert!(DialogFixture::text(&dialog).starts_with("Name      auth█\n"));
    }

    #[test]
    fn a_missing_repo_moves_the_focus_to_the_repo() {
        let (mut dialog, service) = DialogFixture::opened(&ServiceFixture::new());
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);
        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);

        DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(DialogFixture::hints(&dialog), DialogFixture::REPO_HINTS);
    }

    #[test]
    fn an_agent_refusal_keeps_the_focus_on_the_prompt() {
        let agent = Agent::new("plain-agent", "plain-agent");
        let fixture = DialogFixture::launched_in_repo()
            .with_config(ServiceFixture::config().with_agents(vec![agent]));
        let (mut dialog, service) = DialogFixture::focused_on_prompt(&fixture);
        DialogFixture::type_text(&mut dialog, &service, "fix the tests");

        DialogFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert_eq!(DialogFixture::hints(&dialog), DialogFixture::PROMPT_HINTS);
    }

    #[test]
    fn a_failed_create_shows_its_error_in_the_dialog() {
        let fixture = DialogFixture::launched_in_repo();
        let (mut dialog, _service) = DialogFixture::opened(&fixture);

        dialog.fail(DialogFixture::CREATE_FAILURE.to_string());

        assert!(DialogFixture::text(&dialog).contains(DialogFixture::CREATE_FAILURE));
    }

    #[test]
    fn typing_in_the_name_clears_the_error() {
        let (mut dialog, service) = DialogFixture::refused_for_a_missing_name();
        let refused = DialogFixture::shown_error(&dialog);

        DialogFixture::type_text(&mut dialog, &service, "f");

        assert_eq!(refused, NewSessionDialog::NAME_MISSING);
        assert_eq!(DialogFixture::shown_error(&dialog), "");
    }

    #[test]
    fn typing_in_the_prompt_clears_the_error() {
        let (mut dialog, service) = DialogFixture::refused_on_the_prompt();
        let refused = DialogFixture::shown_error(&dialog);

        DialogFixture::type_text(&mut dialog, &service, "s");

        assert_eq!(refused, DialogFixture::PROMPT_REFUSAL);
        assert_eq!(DialogFixture::shown_error(&dialog), "");
    }

    #[test]
    fn opening_the_repo_search_clears_the_error() {
        let (mut dialog, service) = DialogFixture::opened(&ServiceFixture::new());
        DialogFixture::type_text(&mut dialog, &service, "fix-auth");
        DialogFixture::create(&mut dialog, &service);
        let refused = DialogFixture::shown_error(&dialog);

        DialogFixture::type_text(&mut dialog, &service, "g");

        assert_eq!(refused, NewSessionDialog::REPO_MISSING);
        assert_eq!(DialogFixture::shown_error(&dialog), "");
    }

    #[test]
    fn editing_the_repo_search_query_clears_the_error() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        DialogFixture::search(&mut dialog, &service, "g");
        dialog.paste(DialogFixture::TWO_LINE_PATH);
        let refused = DialogFixture::shown_error(&dialog);

        DialogFixture::type_text(&mut dialog, &service, "a");

        assert_eq!(refused, DialogFixture::LINE_BREAK_IN_PATH);
        assert_eq!(DialogFixture::shown_error(&dialog), "");
    }

    #[test]
    fn typing_clears_an_error_reported_through_fail() {
        let (mut dialog, service) = DialogFixture::opened(&DialogFixture::launched_in_repo());
        dialog.fail(DialogFixture::CREATE_FAILURE.to_string());
        let failure = DialogFixture::shown_error(&dialog);

        DialogFixture::type_text(&mut dialog, &service, "f");

        assert_eq!(failure, DialogFixture::CREATE_FAILURE);
        assert_eq!(DialogFixture::shown_error(&dialog), "");
    }

    #[test]
    fn tab_leaves_the_error_on_screen() {
        let (mut dialog, service) = DialogFixture::refused_for_a_missing_name();

        DialogFixture::press(&mut dialog, &service, KeyCode::Tab);

        assert_eq!(
            DialogFixture::shown_error(&dialog),
            NewSessionDialog::NAME_MISSING
        );
    }

    #[test]
    fn an_arrow_key_in_the_prompt_leaves_the_error_on_screen() {
        let (mut dialog, service) = DialogFixture::refused_on_the_prompt();

        DialogFixture::press(&mut dialog, &service, KeyCode::Left);

        assert_eq!(
            DialogFixture::shown_error(&dialog),
            DialogFixture::PROMPT_REFUSAL
        );
    }

    #[test]
    fn alt_j_leaves_the_error_on_screen() {
        let (mut dialog, service) = DialogFixture::refused_for_a_missing_name();

        DialogFixture::toggle_mode(&mut dialog, &service);

        assert_eq!(
            DialogFixture::shown_error(&dialog),
            NewSessionDialog::NAME_MISSING
        );
    }

    #[test]
    fn ctrl_j_through_the_repo_list_leaves_the_error_on_screen() {
        let (mut dialog, service) =
            DialogFixture::opened(&DialogFixture::launched_among_gateways());
        DialogFixture::search(&mut dialog, &service, "gate");
        DialogFixture::settle(&mut dialog, |text| text.contains("20/20 repos under ~"));
        dialog.paste(DialogFixture::TWO_LINE_PATH);

        DialogFixture::control(&mut dialog, &service, 'j');

        assert_eq!(
            DialogFixture::shown_error(&dialog),
            DialogFixture::LINE_BREAK_IN_PATH
        );
    }
}

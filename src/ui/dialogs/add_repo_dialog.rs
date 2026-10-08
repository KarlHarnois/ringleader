use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::Line;

use super::repo_finder::{FinderChoice, RepoFinder};
use super::text_field::{FieldKind, TextField};
use crate::application::{AddRepoRequest, Pending, RepoAddition, SessionService};
use crate::domain::{Repo, RepoName};
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct AddRepoDialog {
    finder: RepoFinder,
    name: Option<TextField>,
    chosen_path: Option<PathBuf>,
    work: Option<AddWork>,
    error: Option<String>,
}

impl AddRepoDialog {
    const CURSOR: char = '█';
    const PATH_LABEL: &'static str = "Repo path: ";
    const NAME_LABEL: &'static str = "Name: ";
    const NOTHING_CHOSEN: &'static str = "pick a repo or type its path";
    const FINDER_HINTS: &'static str =
        "Type to search · ↑ ↓ ctrl-j ctrl-k choose · Enter add · Esc back";
    const NAMING_HINTS: &'static str = "Enter add · Esc back";

    pub fn open(service: &SessionService) -> Self {
        Self {
            finder: RepoFinder::open(service),
            name: None,
            chosen_path: None,
            work: None,
            error: None,
        }
    }

    pub fn hints(&self) -> &'static str {
        match self.name {
            Some(_) => Self::NAMING_HINTS,
            None => Self::FINDER_HINTS,
        }
    }

    pub fn fail(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn handle_key(&mut self, key: &KeyEvent, service: &SessionService) -> bool {
        if self.work.is_some() {
            return true;
        }
        if key.code == KeyCode::Enter {
            self.start(service);
            return true;
        }
        match &mut self.name {
            Some(name) => name.edit(key),
            None => self.finder.handle_key(key),
        }
    }

    pub fn paste(&mut self, text: &str) {
        let pasted = match &mut self.name {
            Some(name) => name.paste(text),
            None => self.finder.paste(text),
        };
        self.error = pasted.err();
    }

    pub fn poll(&mut self, service: &SessionService) -> Option<Repo> {
        self.finder.poll();
        match self.work.as_ref()?.poll()? {
            Finished::Read(Ok(path)) => {
                self.work = Some(AddWork::Adding(
                    service.add_repo(AddRepoRequest::new(&path)),
                ));
                self.chosen_path = Some(path);
                None
            }
            Finished::Added(Ok(
                RepoAddition::Added(repo) | RepoAddition::AlreadyConfigured(repo),
            )) => {
                self.work = None;
                Some(repo)
            }
            Finished::Added(Ok(RepoAddition::NameTaken { name, taken_by })) => {
                self.work = None;
                self.ask_for_name(name.as_str(), &taken_by);
                None
            }
            Finished::Read(Err(error)) | Finished::Added(Err(error)) => {
                self.work = None;
                self.error = Some(error);
                None
            }
        }
    }

    pub fn lines(&self, width: usize, room: usize, theme: &Theme) -> Vec<Line<'static>> {
        let status = self.status_lines(width, theme);
        let mut lines = match &self.name {
            Some(name) => self.naming_lines(name, width),
            None => self
                .finder
                .lines(width, room.saturating_sub(status.len()), theme),
        };
        lines.extend(status);
        lines
    }

    fn status_lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        if let Some(work) = &self.work {
            lines.push(Line::styled(work.busy_message(&self.target()), theme.dim()));
        }
        if let Some(error) = &self.error {
            lines.extend(TextFit::new(error).styled_lines(width, theme.error()));
        }
        lines
    }

    fn naming_lines(&self, name: &TextField, width: usize) -> Vec<Line<'static>> {
        let path_room = width.saturating_sub(Self::PATH_LABEL.len());
        let name_room = width.saturating_sub(Self::NAME_LABEL.len());
        let cursor = self.work.is_none().then_some(Self::CURSOR);
        let path = TextFit::new(&self.target()).left_truncated(path_room);
        vec![
            Line::from(format!("{}{path}", Self::PATH_LABEL)),
            Line::from(format!(
                "{}{}",
                Self::NAME_LABEL,
                name.visible(name_room, cursor)
            )),
        ]
    }

    fn target(&self) -> String {
        match &self.chosen_path {
            Some(path) => path.display().to_string(),
            None => self.finder.query().to_string(),
        }
    }

    fn start(&mut self, service: &SessionService) {
        self.error = None;
        self.work = match (&self.name, &self.chosen_path) {
            (Some(name), Some(path)) => {
                let request = AddRepoRequest::new(path).named(&Self::slug(name));
                Some(AddWork::Adding(service.add_repo(request)))
            }
            _ => self.work_for(self.finder.choice(), service),
        };
    }

    fn work_for(&mut self, choice: FinderChoice, service: &SessionService) -> Option<AddWork> {
        match choice {
            FinderChoice::Repo(path) => {
                let adding = service.add_repo(AddRepoRequest::new(&path));
                self.chosen_path = Some(path);
                Some(AddWork::Adding(adding))
            }
            FinderChoice::TypedPath(typed) => {
                self.chosen_path = None;
                Some(AddWork::Reading(service.read_typed_path(&typed)))
            }
            FinderChoice::Nothing => {
                self.error = Some(Self::NOTHING_CHOSEN.to_string());
                None
            }
        }
    }

    fn slug(name: &TextField) -> String {
        RepoName::from_basename(name.value())
            .map_or_else(|_| name.value().to_string(), |slug| slug.to_string())
    }

    fn ask_for_name(&mut self, name: &str, taken_by: &Repo) {
        self.error = Some(format!(
            "{name} is already used by {}, enter another name",
            taken_by.path().display()
        ));
        if self.name.is_none() {
            self.name = Some(TextField::new(FieldKind::Name).with_value(name));
        }
    }
}

enum AddWork {
    Reading(Pending<PathBuf>),
    Adding(Pending<RepoAddition>),
}

impl AddWork {
    fn poll(&self) -> Option<Finished> {
        let error = |error: crate::application::ServiceError| error.to_string();
        match self {
            Self::Reading(pending) => Some(Finished::Read(pending.poll()?.map_err(error))),
            Self::Adding(pending) => Some(Finished::Added(pending.poll()?.map_err(error))),
        }
    }

    fn busy_message(&self, target: &str) -> String {
        match self {
            Self::Reading(_) => format!("Checking {target}…"),
            Self::Adding(_) => format!("Checking {target} with git and adding it…"),
        }
    }
}

enum Finished {
    Read(Result<PathBuf, String>),
    Added(Result<RepoAddition, String>),
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    use crossterm::event::KeyModifiers;

    use super::*;
    use crate::domain::{Config, RepoName};
    use crate::test_support::{FakeHostProbe, FakeRepoScanner, FakeWorkspace, ServiceFixture};

    struct AddFixture;

    impl AddFixture {
        const NEW_PROJECT: &'static str = "/work/new-project";
        const FOUND: &'static str = "/home/example/hobby/ringleader";
        const SETTLE_LIMIT: Duration = Duration::from_secs(2);

        fn fixture() -> ServiceFixture {
            let found = Repo::discovered(
                RepoName::parse("ringleader").unwrap(),
                Path::new(Self::FOUND),
            );
            ServiceFixture::new()
                .with_host(
                    FakeHostProbe::default()
                        .with_directory(Path::new(Self::NEW_PROJECT))
                        .with_directory(Path::new("/work/plain")),
                )
                .with_scanner(FakeRepoScanner::finding(vec![found]))
        }

        fn with_explicit(name: &str, path: &str) -> ServiceFixture {
            let repo = Repo::explicit(RepoName::parse(name).unwrap(), Path::new(path), None);
            let config: Config = ServiceFixture::config().with_repos(vec![repo]);
            Self::fixture().with_config(config)
        }

        fn opened(service: &SessionService) -> AddRepoDialog {
            let mut dialog = AddRepoDialog::open(service);
            let deadline = Instant::now() + Self::SETTLE_LIMIT;
            while Instant::now() < deadline && Self::text(&dialog).contains("Searching") {
                dialog.poll(service);
                thread::sleep(Duration::from_millis(5));
            }
            dialog
        }

        fn submit(dialog: &mut AddRepoDialog, service: &SessionService, text: &str) {
            for character in text.chars() {
                Self::press(dialog, service, KeyCode::Char(character));
            }
            Self::press(dialog, service, KeyCode::Enter);
        }

        fn press(dialog: &mut AddRepoDialog, service: &SessionService, code: KeyCode) {
            dialog.handle_key(&KeyEvent::new(code, KeyModifiers::NONE), service);
        }

        fn settle(dialog: &mut AddRepoDialog, service: &SessionService) -> Option<Repo> {
            let deadline = Instant::now() + Self::SETTLE_LIMIT;
            while Instant::now() < deadline && dialog.work.is_some() {
                if let Some(repo) = dialog.poll(service) {
                    return Some(repo);
                }
                thread::sleep(Duration::from_millis(5));
            }
            None
        }

        fn text(dialog: &AddRepoDialog) -> String {
            dialog
                .lines(70, 12, &crate::ui::theme::Theme)
                .iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join(" ")
        }
    }

    #[test]
    fn a_found_repo_picked_from_the_finder_is_added_as_an_explicit_entry() {
        let fixture = AddFixture::fixture();
        let service = fixture.service();
        let mut dialog = AddFixture::opened(&service);

        AddFixture::submit(&mut dialog, &service, "ring");
        let added = AddFixture::settle(&mut dialog, &service).unwrap();

        assert_eq!(added.path(), Path::new(AddFixture::FOUND));
        assert_eq!(fixture.config_source().config().repos(), [added]);
    }

    #[test]
    fn enter_with_no_repo_and_no_path_asks_for_one() {
        let fixture = AddFixture::fixture().with_scanner(FakeRepoScanner::finding(Vec::new()));
        let service = fixture.service();
        let mut dialog = AddFixture::opened(&service);

        AddFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert!(AddFixture::text(&dialog).contains("pick a repo or type its path"));
    }

    #[test]
    fn the_hints_switch_to_the_name_prompt_when_a_name_is_taken() {
        let fixture = AddFixture::with_explicit("new-project", "/elsewhere/new-project");
        let service = fixture.service();
        let mut dialog = AddFixture::opened(&service);
        let while_finding = dialog.hints();

        AddFixture::submit(&mut dialog, &service, AddFixture::NEW_PROJECT);
        AddFixture::settle(&mut dialog, &service);

        assert!(while_finding.starts_with("Type to search"));
        assert_eq!(dialog.hints(), "Enter add · Esc back");
    }

    #[test]
    fn a_typed_path_that_matches_no_found_repo_is_added() {
        let fixture = AddFixture::fixture();
        let service = fixture.service();
        let mut dialog = AddFixture::opened(&service);

        AddFixture::submit(&mut dialog, &service, AddFixture::NEW_PROJECT);
        let added = AddFixture::settle(&mut dialog, &service).unwrap();

        assert_eq!(added.name().as_str(), "new-project");
        assert_eq!(fixture.config_source().config().repos(), [added]);
    }

    #[test]
    fn the_dialog_shows_a_busy_message_while_it_checks() {
        let fixture = AddFixture::fixture();
        let service = fixture.service();
        let mut dialog = AddFixture::opened(&service);

        AddFixture::submit(&mut dialog, &service, AddFixture::NEW_PROJECT);

        assert!(AddFixture::text(&dialog).contains("Checking /work/new-project…"));
    }

    #[test]
    fn an_already_configured_repo_returns_its_entry_without_a_write() {
        let fixture = AddFixture::with_explicit("custom-name", AddFixture::NEW_PROJECT);
        let service = fixture.service();
        let mut dialog = AddFixture::opened(&service);

        AddFixture::submit(&mut dialog, &service, AddFixture::NEW_PROJECT);
        let entry = AddFixture::settle(&mut dialog, &service).unwrap();

        assert_eq!(entry.name().as_str(), "custom-name");
        assert_eq!(fixture.config_source().write_count(), 0);
    }

    #[test]
    fn a_taken_name_asks_for_another_prefilled_with_it() {
        let fixture = AddFixture::with_explicit("new-project", "/elsewhere/new-project");
        let service = fixture.service();
        let mut dialog = AddFixture::opened(&service);

        AddFixture::submit(&mut dialog, &service, AddFixture::NEW_PROJECT);
        let first = AddFixture::settle(&mut dialog, &service);

        let text = AddFixture::text(&dialog);
        assert_eq!(first, None);
        assert!(text.contains("Name: new-project█"));
        assert!(
            text.contains(
                "new-project is already used by /elsewhere/new-project, enter another name"
            )
        );
    }

    #[test]
    fn only_a_free_name_is_saved() {
        let fixture = AddFixture::with_explicit("new-project", "/elsewhere/new-project");
        let service = fixture.service();
        let mut dialog = AddFixture::opened(&service);
        AddFixture::submit(&mut dialog, &service, AddFixture::NEW_PROJECT);
        AddFixture::settle(&mut dialog, &service);

        AddFixture::press(&mut dialog, &service, KeyCode::Enter);
        let still_taken = AddFixture::settle(&mut dialog, &service);
        AddFixture::submit(&mut dialog, &service, "-two");
        let added = AddFixture::settle(&mut dialog, &service).unwrap();

        assert_eq!(still_taken, None);
        assert_eq!(added.name().as_str(), "new-project-two");
        assert_eq!(fixture.config_source().write_count(), 1);
    }

    #[test]
    fn a_pasted_name_becomes_a_slug() {
        let fixture = AddFixture::with_explicit("new-project", "/elsewhere/new-project");
        let service = fixture.service();
        let mut dialog = AddFixture::opened(&service);
        AddFixture::submit(&mut dialog, &service, AddFixture::NEW_PROJECT);
        AddFixture::settle(&mut dialog, &service);
        for _ in "new-project".chars() {
            AddFixture::press(&mut dialog, &service, KeyCode::Backspace);
        }

        dialog.paste("Second\nProject");
        AddFixture::press(&mut dialog, &service, KeyCode::Enter);
        let added = AddFixture::settle(&mut dialog, &service).unwrap();

        assert_eq!(added.name().as_str(), "second-project");
    }

    #[test]
    fn a_pasted_path_is_trimmed() {
        let fixture = AddFixture::fixture();
        let service = fixture.service();
        let mut dialog = AddFixture::opened(&service);

        dialog.paste("  /work/new-project\n");
        AddFixture::press(&mut dialog, &service, KeyCode::Enter);

        assert!(AddFixture::settle(&mut dialog, &service).is_some());
    }

    #[test]
    fn a_directory_that_is_not_a_repository_is_refused() {
        let fixture = AddFixture::fixture()
            .with_workspace(FakeWorkspace::default().with_invalid_repo(Path::new("/work/plain")));
        let service = fixture.service();
        let mut dialog = AddFixture::opened(&service);

        AddFixture::submit(&mut dialog, &service, "/work/plain");

        assert_eq!(AddFixture::settle(&mut dialog, &service), None);
        assert!(AddFixture::text(&dialog).contains("not a git repository"));
    }

    #[test]
    fn a_missing_path_is_refused() {
        let fixture = AddFixture::fixture();
        let service = fixture.service();
        let mut dialog = AddFixture::opened(&service);

        AddFixture::submit(&mut dialog, &service, "/nowhere");

        assert_eq!(AddFixture::settle(&mut dialog, &service), None);
        assert!(AddFixture::text(&dialog).contains("no such directory: /nowhere"));
    }
}

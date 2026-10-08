use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::Line;

use super::chosen_repo::ChosenRepo;
use super::repo_finder::RepoFinder;
use crate::application::SessionService;
use crate::domain::Repo;
use crate::ui::theme::Theme;

pub struct RepoPicker {
    chosen: ChosenRepo,
    finder: RepoFinder,
    searching: bool,
}

impl RepoPicker {
    const FIELD_HINTS: &'static str =
        "Type to find another repo · Tab next field · Enter create · Esc cancel";

    pub fn open(service: &SessionService) -> Self {
        Self {
            chosen: ChosenRepo::open(service),
            finder: RepoFinder::open(service),
            searching: false,
        }
    }

    pub fn has_open_form(&self) -> bool {
        self.searching
    }

    pub fn hints(&self) -> &'static str {
        if self.searching {
            self.finder.hints()
        } else {
            Self::FIELD_HINTS
        }
    }

    pub fn selected_repo(&self) -> Option<&Repo> {
        self.chosen.repo()
    }

    pub fn summary(&self) -> String {
        self.chosen.text()
    }

    pub fn poll(&mut self) {
        self.chosen.poll();
        if let Some(repo) = self.finder.poll() {
            self.choose(repo);
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent, service: &SessionService) -> bool {
        if self.searching {
            self.search_key(key, service);
            return true;
        }
        if !Self::starts_search(key) {
            return false;
        }
        self.finder.begin(key);
        self.searching = true;
        true
    }

    pub fn paste(&mut self, text: &str) -> Result<(), String> {
        self.finder.paste(text)
    }

    pub fn search_lines(&self, width: usize, room: usize, theme: &Theme) -> Vec<Line<'static>> {
        self.finder.lines(width, room, theme)
    }

    fn search_key(&mut self, key: &KeyEvent, service: &SessionService) {
        match key.code {
            KeyCode::Esc => self.searching = false,
            KeyCode::Enter => {
                if let Some(repo) = self.finder.pick(service) {
                    self.choose(repo);
                }
            }
            _ => {
                self.finder.handle_key(key);
            }
        }
    }

    fn choose(&mut self, repo: Repo) {
        self.chosen.set(repo);
        self.searching = false;
    }

    fn starts_search(key: &KeyEvent) -> bool {
        let typed_text = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        matches!(key.code, KeyCode::Char(character) if typed_text && !character.is_whitespace())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::domain::RepoName;
    use crate::test_support::{FakeHostProbe, FakeRepoScanner, ServiceFixture};

    struct PickerFixture;

    impl PickerFixture {
        const SETTLE_LIMIT: Duration = Duration::from_secs(2);
        const FOUND: &'static str = "/home/example/work/api-gateway";
        const TYPED: &'static str = "/srv/tools";

        fn fixture() -> ServiceFixture {
            let found = Repo::discovered(
                RepoName::parse("api-gateway").unwrap(),
                Path::new(Self::FOUND),
            );
            ServiceFixture::new()
                .with_scanner(
                    FakeRepoScanner::finding(vec![found])
                        .inside_repo(Path::new(ServiceFixture::REPO)),
                )
                .with_host(FakeHostProbe::default().with_directory(Path::new(Self::TYPED)))
        }

        fn opened(service: &SessionService) -> RepoPicker {
            let mut picker = RepoPicker::open(service);
            Self::settle(&mut picker, |picker| {
                !picker.summary().starts_with("Finding")
                    && !Self::search_text(picker).contains("Searching")
            });
            picker
        }

        fn settle(picker: &mut RepoPicker, done: impl Fn(&RepoPicker) -> bool) {
            let deadline = Instant::now() + Self::SETTLE_LIMIT;
            while Instant::now() < deadline && !done(picker) {
                picker.poll();
                thread::sleep(Duration::from_millis(5));
            }
        }

        fn type_text(picker: &mut RepoPicker, service: &SessionService, text: &str) {
            for character in text.chars() {
                Self::press(picker, service, KeyCode::Char(character));
            }
        }

        fn press(picker: &mut RepoPicker, service: &SessionService, code: KeyCode) -> bool {
            picker.handle_key(&KeyEvent::new(code, KeyModifiers::NONE), service)
        }

        fn search_text(picker: &RepoPicker) -> String {
            picker
                .search_lines(60, 10, &Theme)
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
    }

    #[test]
    fn the_field_says_it_is_finding_the_default_until_it_resolves() {
        let picker = RepoPicker::open(&PickerFixture::fixture().service());

        assert_eq!(picker.summary(), "Finding the repo you started in…");
    }

    #[test]
    fn the_field_shows_the_repo_ringleader_started_in() {
        let picker = PickerFixture::opened(&PickerFixture::fixture().service());

        assert_eq!(picker.summary(), "example-project · /repos/example-project");
        assert_eq!(
            picker.selected_repo().unwrap().path(),
            Path::new(ServiceFixture::REPO)
        );
    }

    #[test]
    fn without_a_default_the_field_asks_for_a_search() {
        let picker = PickerFixture::opened(&ServiceFixture::new().service());

        assert_eq!(picker.summary(), "none, type to find one");
        assert_eq!(picker.selected_repo(), None);
    }

    #[test]
    fn typing_opens_the_search_with_that_character() {
        let service = PickerFixture::fixture().service();
        let mut picker = PickerFixture::opened(&service);

        let consumed = PickerFixture::press(&mut picker, &service, KeyCode::Char('r'));

        assert!(consumed);
        assert!(picker.has_open_form());
        assert!(PickerFixture::search_text(&picker).starts_with("Find: r█"));
    }

    #[test]
    fn keys_that_type_nothing_leave_the_search_closed() {
        let service = PickerFixture::fixture().service();
        let mut picker = PickerFixture::opened(&service);

        let consumed = [KeyCode::Tab, KeyCode::Enter, KeyCode::Char(' ')]
            .map(|code| PickerFixture::press(&mut picker, &service, code));

        assert_eq!(consumed, [false, false, false]);
        assert!(!picker.has_open_form());
    }

    #[test]
    fn enter_on_a_match_picks_it_without_writing_the_config() {
        let fixture = PickerFixture::fixture();
        let service = fixture.service();
        let mut picker = PickerFixture::opened(&service);

        PickerFixture::type_text(&mut picker, &service, "gate");
        PickerFixture::press(&mut picker, &service, KeyCode::Enter);

        assert!(!picker.has_open_form());
        assert_eq!(picker.summary(), "api-gateway · ~/work/api-gateway");
        assert_eq!(fixture.config_source().write_count(), 0);
    }

    #[test]
    fn esc_closes_the_search_and_keeps_the_previous_repo() {
        let service = PickerFixture::fixture().service();
        let mut picker = PickerFixture::opened(&service);
        PickerFixture::type_text(&mut picker, &service, "gate");

        PickerFixture::press(&mut picker, &service, KeyCode::Esc);

        assert!(!picker.has_open_form());
        assert_eq!(
            picker.selected_repo().unwrap().path(),
            Path::new(ServiceFixture::REPO)
        );
    }

    #[test]
    fn a_typed_repo_path_is_picked_after_the_check() {
        let service = PickerFixture::fixture().service();
        let mut picker = PickerFixture::opened(&service);
        PickerFixture::type_text(&mut picker, &service, PickerFixture::TYPED);

        PickerFixture::press(&mut picker, &service, KeyCode::Enter);
        PickerFixture::settle(&mut picker, |picker| !picker.has_open_form());

        assert_eq!(
            picker.selected_repo().unwrap().path(),
            Path::new(PickerFixture::TYPED)
        );
    }

    #[test]
    fn the_hints_follow_the_search() {
        let service = PickerFixture::fixture().service();
        let mut picker = PickerFixture::opened(&service);
        let on_the_field = picker.hints();

        PickerFixture::press(&mut picker, &service, KeyCode::Char('a'));

        assert!(on_the_field.starts_with("Type to find another repo"));
        assert!(picker.hints().ends_with("Enter pick · Esc back"));
    }
}

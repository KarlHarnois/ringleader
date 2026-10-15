use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Style;
use ratatui::text::Line;

use super::chosen_repo::ChosenRepo;
use crate::application::SessionService;
use crate::domain::Repo;
use crate::ui::dialogs::repo_search::repo_finder::RepoFinder;
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct RepoPicker {
    chosen: ChosenRepo,
    finder: RepoFinder,
    searching: bool,
}

impl RepoPicker {
    pub fn open(service: &SessionService) -> Self {
        Self {
            chosen: ChosenRepo::open(service),
            finder: RepoFinder::open(service),
            searching: false,
        }
    }

    pub fn hints(&self) -> Option<&'static str> {
        self.searching.then(|| self.finder.hints())
    }

    pub fn selected_repo(&self) -> Option<&Repo> {
        self.chosen.repo()
    }

    pub fn poll(&mut self) -> PickerStep {
        self.chosen.poll();
        match self.finder.poll() {
            Some(repo) => self.choose(repo),
            None => PickerStep::Ignored,
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent, service: &SessionService) -> PickerStep {
        if self.searching {
            return self.search_key(key, service);
        }
        if !Self::starts_search(key) {
            return PickerStep::Ignored;
        }
        self.finder.begin(key);
        self.searching = true;
        PickerStep::Handled
    }

    pub fn paste(&mut self, text: &str) -> Result<(), String> {
        if self.searching {
            self.finder.paste(text)
        } else {
            Ok(())
        }
    }

    pub fn lines(
        &self,
        width: usize,
        room: usize,
        summary_style: Style,
        theme: &Theme,
    ) -> Vec<Line<'static>> {
        if self.searching {
            return self.finder.lines(width, room, theme);
        }
        let summary = TextFit::new(&self.chosen.text()).right_truncated(width);
        vec![Line::styled(summary, summary_style)]
    }

    pub fn fewest_rows(&self, width: usize, theme: &Theme) -> usize {
        let no_spare_rows = 0;
        self.lines(width, no_spare_rows, theme.base(), theme).len()
    }

    fn search_key(&mut self, key: &KeyEvent, service: &SessionService) -> PickerStep {
        match key.code {
            KeyCode::Esc => self.searching = false,
            KeyCode::Enter => {
                if let Some(repo) = self.finder.pick(service) {
                    return self.choose(repo);
                }
            }
            _ => {
                self.finder.handle_key(key);
            }
        }
        PickerStep::Handled
    }

    fn choose(&mut self, repo: Repo) -> PickerStep {
        self.chosen.set(repo);
        self.searching = false;
        PickerStep::Picked
    }

    fn starts_search(key: &KeyEvent) -> bool {
        let typed_text = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        matches!(key.code, KeyCode::Char(character) if typed_text && !character.is_whitespace())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerStep {
    Ignored,
    Handled,
    Picked,
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
                !Self::text(picker).starts_with("Finding")
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

        fn poll_until_picked(picker: &mut RepoPicker) -> PickerStep {
            let deadline = Instant::now() + Self::SETTLE_LIMIT;
            let mut step = picker.poll();
            while Instant::now() < deadline && step != PickerStep::Picked {
                thread::sleep(Duration::from_millis(5));
                step = picker.poll();
            }
            step
        }

        fn type_text(picker: &mut RepoPicker, service: &SessionService, text: &str) {
            for character in text.chars() {
                Self::press(picker, service, KeyCode::Char(character));
            }
        }

        fn press(picker: &mut RepoPicker, service: &SessionService, code: KeyCode) -> PickerStep {
            picker.handle_key(&KeyEvent::new(code, KeyModifiers::NONE), service)
        }

        fn is_searching(picker: &RepoPicker) -> bool {
            picker.hints().is_some()
        }

        fn text(picker: &RepoPicker) -> String {
            picker
                .lines(60, 10, Theme.base(), &Theme)
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

        assert_eq!(
            PickerFixture::text(&picker),
            "Finding the repo you started in…"
        );
    }

    #[test]
    fn the_field_shows_the_repo_ringleader_started_in() {
        let picker = PickerFixture::opened(&PickerFixture::fixture().service());

        assert_eq!(PickerFixture::text(&picker), "example-project");
        assert_eq!(
            picker.selected_repo().unwrap().path(),
            Path::new(ServiceFixture::REPO)
        );
    }

    #[test]
    fn without_a_default_the_field_asks_for_a_search() {
        let picker = PickerFixture::opened(&ServiceFixture::new().service());

        assert_eq!(PickerFixture::text(&picker), "none, type to find one");
        assert_eq!(picker.selected_repo(), None);
    }

    #[test]
    fn typing_opens_the_search_with_that_character() {
        let service = PickerFixture::fixture().service();
        let mut picker = PickerFixture::opened(&service);

        let step = PickerFixture::press(&mut picker, &service, KeyCode::Char('r'));

        assert_eq!(step, PickerStep::Handled);
        assert!(PickerFixture::is_searching(&picker));
        assert!(PickerFixture::text(&picker).starts_with("Find: r█"));
    }

    #[test]
    fn keys_that_type_nothing_leave_the_search_closed() {
        let service = PickerFixture::fixture().service();
        let mut picker = PickerFixture::opened(&service);

        let steps = [KeyCode::Tab, KeyCode::Enter, KeyCode::Char(' ')]
            .map(|code| PickerFixture::press(&mut picker, &service, code));

        assert_eq!(steps, [PickerStep::Ignored; 3]);
        assert!(!PickerFixture::is_searching(&picker));
    }

    #[test]
    fn enter_on_a_match_picks_it_without_writing_the_config() {
        let fixture = PickerFixture::fixture();
        let service = fixture.service();
        let mut picker = PickerFixture::opened(&service);

        PickerFixture::type_text(&mut picker, &service, "gate");
        PickerFixture::settle(&mut picker, |picker| {
            PickerFixture::text(picker).contains("1/1 repos under ~")
        });
        let step = PickerFixture::press(&mut picker, &service, KeyCode::Enter);

        assert_eq!(step, PickerStep::Picked);
        assert!(!PickerFixture::is_searching(&picker));
        assert_eq!(PickerFixture::text(&picker), "api-gateway");
        assert_eq!(fixture.config_source().write_count(), 0);
    }

    #[test]
    fn esc_closes_the_search_and_keeps_the_previous_repo() {
        let service = PickerFixture::fixture().service();
        let mut picker = PickerFixture::opened(&service);
        PickerFixture::type_text(&mut picker, &service, "gate");

        PickerFixture::press(&mut picker, &service, KeyCode::Esc);

        assert!(!PickerFixture::is_searching(&picker));
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

        let at_once = PickerFixture::press(&mut picker, &service, KeyCode::Enter);
        let checked = PickerFixture::poll_until_picked(&mut picker);

        assert_eq!(at_once, PickerStep::Handled);
        assert_eq!(checked, PickerStep::Picked);
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

        assert_eq!(on_the_field, None);
        assert!(
            picker
                .hints()
                .is_some_and(|hints| hints.ends_with("Enter pick · Esc back"))
        );
    }
}

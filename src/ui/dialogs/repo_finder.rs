use crossterm::event::KeyEvent;
use ratatui::text::Line;

use super::home_path::HomePath;
use super::match_list::MatchList;
use super::repo_filter::RepoFilter;
use super::repo_scan::RepoScan;
use super::search_query::SearchQuery;
use super::text_field::FieldKind;
use super::typed_repo_check::TypedRepoCheck;
use crate::application::SessionService;
use crate::domain::Repo;
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct RepoFinder {
    query: SearchQuery,
    scan: RepoScan,
    filter: RepoFilter,
    matches: MatchList,
    check: Option<TypedRepoCheck>,
}

impl RepoFinder {
    const SEARCHING: &'static str = "Searching ~ for repos…";
    const NO_MATCH: &'static str = "no repo matches, Enter picks the typed path";
    const HINTS: &'static str = "Type to search · ↑ ↓ Ctrl-J Ctrl-K choose · Enter pick · Esc back";

    pub fn open(service: &SessionService) -> Self {
        Self {
            query: SearchQuery::new(FieldKind::Path),
            scan: RepoScan::start(service),
            filter: RepoFilter::new(HomePath::new(service.home_directory())),
            matches: MatchList::default(),
            check: None,
        }
    }

    pub fn hints(&self) -> &'static str {
        Self::HINTS
    }

    pub fn begin(&mut self, first_key: &KeyEvent) {
        self.query = SearchQuery::new(FieldKind::Path);
        self.query.edit(first_key);
        self.query_changed();
    }

    pub fn poll(&mut self) -> Option<Repo> {
        if self.scan.finish_if_done() {
            self.filter.load(self.scan.repos());
            self.rank();
        }
        let picked = self.check.as_mut()?.poll();
        if picked.is_some() {
            self.check = None;
        }
        picked
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        if self.matches.move_cursor(key) {
            return true;
        }
        let edited = self.query.edit(key);
        if edited {
            self.query_changed();
        }
        edited
    }

    pub fn paste(&mut self, text: &str) -> Result<(), String> {
        self.query.paste(text)?;
        self.query_changed();
        Ok(())
    }

    pub fn pick(&mut self, service: &SessionService) -> Option<Repo> {
        if let Some(index) = self.matches.selected() {
            return Some(self.scan.repos()[index].clone());
        }
        self.check = Some(TypedRepoCheck::start(service, self.typed_path()?));
        None
    }

    pub fn lines(&self, width: usize, room: usize, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = vec![self.query.line(width)];
        lines.extend(self.summary_lines(width, theme));
        let footer = self.footer(width, theme);
        let rows = room.saturating_sub(lines.len() + footer.len()).max(1);
        lines.extend(self.match_lines(width, rows, theme));
        lines.extend(footer);
        lines
    }

    fn typed_path(&self) -> Option<&str> {
        let query = self.query.value();
        let offers_typed_path = self.matches.is_empty() && !query.trim().is_empty();
        offers_typed_path.then_some(query)
    }

    fn query_changed(&mut self) {
        self.check = None;
        self.rank();
    }

    fn rank(&mut self) {
        self.matches.replace(self.filter.ranked(self.query.value()));
    }

    fn summary_lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        if self.scan.is_scanning() {
            return vec![Line::styled(Self::SEARCHING, theme.dim())];
        }
        let notices = self.scan.notices().into_iter();
        let mut lines: Vec<Line<'static>> = notices
            .flat_map(|notice| TextFit::new(&notice).styled_lines(width, theme.dim()))
            .collect();
        match self.scan.problem() {
            Some(problem) => {
                lines.extend(TextFit::new(&problem).styled_lines(width, theme.error()))
            }
            None => lines.insert(0, Line::styled(self.count(), theme.dim())),
        }
        lines
    }

    fn count(&self) -> String {
        let found = self.scan.repos().len();
        format!("{}/{found} repos under ~", self.matches.len())
    }

    fn match_lines(&self, width: usize, rows: usize, theme: &Theme) -> Vec<Line<'static>> {
        self.matches.lines(rows, theme, |index| {
            TextFit::new(self.filter.shown_path(index)).left_truncated(width)
        })
    }

    fn footer(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        if let Some(check) = &self.check {
            return check.lines(width, theme);
        }
        if self.scan.is_scanning() || self.typed_path().is_none() {
            return Vec::new();
        }
        vec![Line::styled(Self::NO_MATCH, theme.dim())]
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::{Duration, Instant};

    use crossterm::event::{KeyCode, KeyModifiers};

    use super::*;
    use crate::application::ports::ScanError;
    use crate::domain::RepoName;
    use crate::test_support::{FakeHostProbe, FakeRepoScanner, FakeWorkspace, ServiceFixture};

    struct FinderFixture;

    impl FinderFixture {
        const SETTLE_LIMIT: Duration = Duration::from_secs(2);
        const RINGLEADER: &'static str = "/home/example/hobby/ringleader";
        const GATEWAY: &'static str = "/home/example/work/api-gateway";
        const NOTES: &'static str = "/home/example/notes";
        const TOOLS: &'static str = "/srv/tools";
        const PLAIN: &'static str = "/srv/plain";

        fn fixture() -> ServiceFixture {
            let repos = [Self::RINGLEADER, Self::NOTES, Self::GATEWAY]
                .iter()
                .map(|path| Self::repo(path))
                .collect();
            ServiceFixture::new()
                .with_scanner(FakeRepoScanner::finding(repos))
                .with_host(
                    FakeHostProbe::default()
                        .with_directory(Path::new(Self::TOOLS))
                        .with_directory(Path::new(Self::PLAIN)),
                )
                .with_workspace(FakeWorkspace::default().with_invalid_repo(Path::new(Self::PLAIN)))
        }

        fn repo(path: &str) -> Repo {
            let path = Path::new(path);
            Repo::discovered(RepoName::of_directory(path).unwrap(), path)
        }

        fn opened(service: &SessionService) -> RepoFinder {
            let mut finder = RepoFinder::open(service);
            Self::settle(&mut finder, |text| !text.contains("Searching"));
            finder
        }

        fn settle(finder: &mut RepoFinder, done: impl Fn(&str) -> bool) -> Option<Repo> {
            let deadline = Instant::now() + Self::SETTLE_LIMIT;
            while Instant::now() < deadline && !done(&Self::text(finder)) {
                if let Some(repo) = finder.poll() {
                    return Some(repo);
                }
                thread::sleep(Duration::from_millis(5));
            }
            None
        }

        fn type_text(finder: &mut RepoFinder, text: &str) {
            for character in text.chars() {
                Self::press(finder, KeyCode::Char(character), KeyModifiers::NONE);
            }
        }

        fn press(finder: &mut RepoFinder, code: KeyCode, modifiers: KeyModifiers) {
            finder.handle_key(&KeyEvent::new(code, modifiers));
        }

        fn picked_path(finder: &mut RepoFinder, service: &SessionService) -> Option<PathBuf> {
            finder.pick(service).map(|repo| repo.path().to_path_buf())
        }

        fn text(finder: &RepoFinder) -> String {
            finder
                .lines(60, 10, &Theme)
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
    fn the_finder_says_it_is_searching_until_the_scan_finishes() {
        let fixture = ServiceFixture::new()
            .with_scanner(FakeRepoScanner::finding(vec![ServiceFixture::repo()]).blocked());

        let finder = RepoFinder::open(&fixture.service());

        assert!(FinderFixture::text(&finder).contains("Searching ~ for repos…"));
        fixture.scanner().release();
    }

    #[test]
    fn the_finder_lists_every_repo_under_home_before_any_typing() {
        let finder = FinderFixture::opened(&FinderFixture::fixture().service());

        assert_eq!(
            FinderFixture::text(&finder),
            "Find: █\n3/3 repos under ~\n~/hobby/ringleader\n~/notes\n~/work/api-gateway"
        );
    }

    #[test]
    fn typing_filters_the_list_and_updates_the_count() {
        let service = FinderFixture::fixture().service();
        let mut finder = FinderFixture::opened(&service);

        FinderFixture::type_text(&mut finder, "ring");

        assert_eq!(
            FinderFixture::text(&finder),
            "Find: ring█\n1/3 repos under ~\n~/hobby/ringleader"
        );
        assert_eq!(
            FinderFixture::picked_path(&mut finder, &service),
            Some(PathBuf::from(FinderFixture::RINGLEADER))
        );
    }

    #[test]
    fn beginning_a_search_starts_a_fresh_query_with_the_first_key() {
        let mut finder = FinderFixture::opened(&FinderFixture::fixture().service());
        FinderFixture::type_text(&mut finder, "old");

        finder.begin(&KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));

        assert!(FinderFixture::text(&finder).starts_with("Find: r█"));
    }

    #[test]
    fn control_w_deletes_the_last_query_word_and_ranks_again() {
        let mut finder = FinderFixture::opened(&FinderFixture::fixture().service());
        FinderFixture::type_text(&mut finder, "ring zzz");

        FinderFixture::press(&mut finder, KeyCode::Char('w'), KeyModifiers::CONTROL);

        assert_eq!(
            FinderFixture::text(&finder),
            "Find: ring █\n1/3 repos under ~\n~/hobby/ringleader"
        );
    }

    #[test]
    fn j_and_k_are_typed_into_the_query() {
        let mut finder = FinderFixture::opened(&FinderFixture::fixture().service());

        FinderFixture::type_text(&mut finder, "jk");

        assert!(FinderFixture::text(&finder).starts_with("Find: jk█"));
    }

    #[test]
    fn down_and_control_j_move_to_the_next_match() {
        let service = FinderFixture::fixture().service();
        let mut finder = FinderFixture::opened(&service);

        FinderFixture::press(&mut finder, KeyCode::Down, KeyModifiers::NONE);
        FinderFixture::press(&mut finder, KeyCode::Char('j'), KeyModifiers::CONTROL);

        assert_eq!(
            FinderFixture::picked_path(&mut finder, &service),
            Some(PathBuf::from(FinderFixture::GATEWAY))
        );
    }

    #[test]
    fn up_and_control_k_move_back_and_stop_at_the_first_match() {
        let service = FinderFixture::fixture().service();
        let mut finder = FinderFixture::opened(&service);
        FinderFixture::press(&mut finder, KeyCode::Down, KeyModifiers::NONE);

        FinderFixture::press(&mut finder, KeyCode::Char('k'), KeyModifiers::CONTROL);
        FinderFixture::press(&mut finder, KeyCode::Up, KeyModifiers::NONE);

        assert_eq!(
            FinderFixture::picked_path(&mut finder, &service),
            Some(PathBuf::from(FinderFixture::RINGLEADER))
        );
    }

    #[test]
    fn a_query_without_matches_offers_the_typed_path() {
        let mut finder = FinderFixture::opened(&FinderFixture::fixture().service());

        FinderFixture::type_text(&mut finder, FinderFixture::TOOLS);

        assert!(
            FinderFixture::text(&finder).contains("no repo matches, Enter picks the typed path")
        );
    }

    #[test]
    fn a_typed_repo_path_is_picked_once_git_confirms_it() {
        let service = FinderFixture::fixture().service();
        let mut finder = FinderFixture::opened(&service);
        FinderFixture::type_text(&mut finder, FinderFixture::TOOLS);

        let at_once = finder.pick(&service);
        let checked = FinderFixture::settle(&mut finder, |_| false);

        assert_eq!(at_once, None);
        assert_eq!(checked.unwrap().path(), Path::new(FinderFixture::TOOLS));
    }

    #[test]
    fn a_typed_plain_directory_shows_why_it_was_refused() {
        let service = FinderFixture::fixture().service();
        let mut finder = FinderFixture::opened(&service);
        FinderFixture::type_text(&mut finder, FinderFixture::PLAIN);

        finder.pick(&service);
        let checked =
            FinderFixture::settle(&mut finder, |text| text.contains("not a git repository"));

        assert_eq!(checked, None);
        assert!(FinderFixture::text(&finder).contains("not a git repository"));
    }

    #[test]
    fn editing_the_query_clears_a_refusal() {
        let service = FinderFixture::fixture().service();
        let mut finder = FinderFixture::opened(&service);
        FinderFixture::type_text(&mut finder, FinderFixture::PLAIN);
        finder.pick(&service);
        FinderFixture::settle(&mut finder, |text| text.contains("not a git repository"));

        FinderFixture::press(&mut finder, KeyCode::Backspace, KeyModifiers::NONE);

        assert!(!FinderFixture::text(&finder).contains("not a git repository"));
    }

    #[test]
    fn an_empty_home_offers_nothing_until_a_path_is_typed() {
        let fixture = ServiceFixture::new().with_scanner(FakeRepoScanner::finding(Vec::new()));
        let service = fixture.service();
        let mut finder = FinderFixture::opened(&service);

        assert!(FinderFixture::text(&finder).contains("no repos found in /home/example"));
        assert_eq!(finder.pick(&service), None);
    }

    #[test]
    fn a_failed_search_shows_its_error() {
        let fixture = ServiceFixture::new().with_scanner(
            FakeRepoScanner::finding(Vec::new()).failing(ScanError::Unreadable {
                path: PathBuf::from("/home/example"),
                detail: "permission denied".to_string(),
            }),
        );

        let finder = FinderFixture::opened(&fixture.service());

        assert!(
            FinderFixture::text(&finder).contains("cannot list /home/example: permission denied")
        );
    }

    #[test]
    fn a_pasted_query_filters_the_list() {
        let service = FinderFixture::fixture().service();
        let mut finder = FinderFixture::opened(&service);

        finder.paste(" gateway\n").unwrap();

        assert_eq!(
            FinderFixture::picked_path(&mut finder, &service),
            Some(PathBuf::from(FinderFixture::GATEWAY))
        );
    }

    #[test]
    fn the_list_scrolls_to_keep_the_cursor_in_view() {
        let mut finder = FinderFixture::opened(&FinderFixture::fixture().service());
        FinderFixture::press(&mut finder, KeyCode::Down, KeyModifiers::NONE);
        FinderFixture::press(&mut finder, KeyCode::Down, KeyModifiers::NONE);

        let shown = finder.lines(60, 3, &Theme);

        assert_eq!(shown.len(), 3);
        assert_eq!(shown[2].spans[0].content, "~/work/api-gateway");
    }
}

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::Line;

use super::repo_filter::RepoFilter;
use super::repo_scan::RepoScan;
use super::text_field::{FieldKind, TextField};
use crate::application::SessionService;
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct RepoFinder {
    query: TextField,
    scan: RepoScan,
    filter: RepoFilter,
    matches: Vec<usize>,
    cursor: usize,
}

impl RepoFinder {
    const LABEL: &'static str = "Find: ";
    const CURSOR: char = '█';
    const SEARCHING: &'static str = "Searching ~ for repos…";
    const NO_MATCH: &'static str = "no repo matches, Enter adds the typed path";

    pub fn open(service: &SessionService) -> Self {
        Self {
            query: TextField::new(FieldKind::Path),
            scan: RepoScan::home(service),
            filter: RepoFilter::default(),
            matches: Vec::new(),
            cursor: 0,
        }
    }

    pub fn query(&self) -> &str {
        self.query.value()
    }

    pub fn poll(&mut self) {
        if !self.scan.finish_if_done() {
            return;
        }
        if let Some(found) = self.scan.found() {
            self.filter = RepoFilter::new(found.repos(), found.scope().root());
        }
        self.rank();
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        if let Some(offset) = Self::cursor_offset(key) {
            self.move_cursor(offset);
            return true;
        }
        let edited = self.query.edit(key);
        if edited {
            self.rank();
        }
        edited
    }

    pub fn paste(&mut self, text: &str) -> Result<(), String> {
        self.query.paste(text)?;
        self.rank();
        Ok(())
    }

    pub fn choice(&self) -> FinderChoice {
        match self.matches.get(self.cursor) {
            Some(&index) => FinderChoice::Repo(self.scan.repos()[index].path().to_path_buf()),
            None if self.query().trim().is_empty() => FinderChoice::Nothing,
            None => FinderChoice::TypedPath(self.query().to_string()),
        }
    }

    pub fn lines(&self, width: usize, room: usize, theme: &Theme) -> Vec<Line<'static>> {
        let field_room = width.saturating_sub(Self::LABEL.len());
        let shown_query = self.query.visible(field_room, Some(Self::CURSOR));
        let mut lines = vec![Line::from(format!("{}{shown_query}", Self::LABEL))];
        lines.extend(self.summary_lines(width, theme));
        let footer = self.footer(theme);
        let rows = room.saturating_sub(lines.len() + footer.len()).max(1);
        lines.extend(self.match_lines(width, rows, theme));
        lines.extend(footer);
        lines
    }

    fn cursor_offset(key: &KeyEvent) -> Option<isize> {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Up => Some(-1),
            KeyCode::Down => Some(1),
            KeyCode::Char('k' | 'p') if control => Some(-1),
            KeyCode::Char('j' | 'n') if control => Some(1),
            _ => None,
        }
    }

    fn move_cursor(&mut self, offset: isize) {
        let last = self.matches.len().saturating_sub(1);
        self.cursor = self.cursor.saturating_add_signed(offset).min(last);
    }

    fn rank(&mut self) {
        self.matches = self.filter.ranked(self.query.value());
        self.cursor = 0;
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
        format!(
            "{}/{} repos under ~",
            self.matches.len(),
            self.filter.count()
        )
    }

    fn match_lines(&self, width: usize, rows: usize, theme: &Theme) -> Vec<Line<'static>> {
        let first = (self.cursor + 1).saturating_sub(rows);
        let visible = self.matches.iter().enumerate().skip(first).take(rows);
        visible
            .map(|(position, &index)| {
                let path = TextFit::new(self.filter.shown_path(index)).left_truncated(width);
                let style = if position == self.cursor {
                    theme.selected()
                } else {
                    theme.base()
                };
                Line::styled(path, style)
            })
            .collect()
    }

    fn footer(&self, theme: &Theme) -> Vec<Line<'static>> {
        let offers_typed_path = matches!(self.choice(), FinderChoice::TypedPath(_));
        if self.scan.is_scanning() || !offers_typed_path {
            return Vec::new();
        }
        vec![Line::styled(Self::NO_MATCH, theme.dim())]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinderChoice {
    Repo(PathBuf),
    TypedPath(String),
    Nothing,
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::application::ports::ScanError;
    use crate::domain::{Repo, RepoName};
    use crate::test_support::{FakeRepoScanner, ServiceFixture};

    struct FinderFixture;

    impl FinderFixture {
        const SETTLE_LIMIT: Duration = Duration::from_secs(2);
        const RINGLEADER: &'static str = "/home/example/hobby/ringleader";
        const GATEWAY: &'static str = "/home/example/work/api-gateway";
        const NOTES: &'static str = "/home/example/notes";

        fn fixture() -> ServiceFixture {
            let repos = [Self::RINGLEADER, Self::NOTES, Self::GATEWAY]
                .iter()
                .map(|path| Self::repo(path))
                .collect();
            ServiceFixture::new().with_scanner(FakeRepoScanner::finding(repos))
        }

        fn repo(path: &str) -> Repo {
            let path = Path::new(path);
            let basename = path.file_name().unwrap().to_string_lossy();
            Repo::discovered(RepoName::from_basename(&basename).unwrap(), path)
        }

        fn opened(fixture: &ServiceFixture) -> RepoFinder {
            let mut finder = RepoFinder::open(&fixture.service());
            let deadline = Instant::now() + Self::SETTLE_LIMIT;
            while Instant::now() < deadline && Self::text(&finder).contains("Searching") {
                finder.poll();
                thread::sleep(Duration::from_millis(5));
            }
            finder
        }

        fn type_text(finder: &mut RepoFinder, text: &str) {
            for character in text.chars() {
                Self::press(finder, KeyCode::Char(character), KeyModifiers::NONE);
            }
        }

        fn press(finder: &mut RepoFinder, code: KeyCode, modifiers: KeyModifiers) {
            finder.handle_key(&KeyEvent::new(code, modifiers));
        }

        fn chosen(path: &str) -> FinderChoice {
            FinderChoice::Repo(PathBuf::from(path))
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
        let finder = FinderFixture::opened(&FinderFixture::fixture());

        assert_eq!(
            FinderFixture::text(&finder),
            "Find: █\n3/3 repos under ~\n~/hobby/ringleader\n~/notes\n~/work/api-gateway"
        );
    }

    #[test]
    fn typing_filters_the_list_and_updates_the_count() {
        let mut finder = FinderFixture::opened(&FinderFixture::fixture());

        FinderFixture::type_text(&mut finder, "ring");

        assert_eq!(
            FinderFixture::text(&finder),
            "Find: ring█\n1/3 repos under ~\n~/hobby/ringleader"
        );
        assert_eq!(
            finder.choice(),
            FinderFixture::chosen(FinderFixture::RINGLEADER)
        );
    }

    #[test]
    fn j_and_k_are_typed_into_the_query() {
        let mut finder = FinderFixture::opened(&FinderFixture::fixture());

        FinderFixture::type_text(&mut finder, "jk");

        assert!(FinderFixture::text(&finder).starts_with("Find: jk█"));
    }

    #[test]
    fn down_and_control_j_move_to_the_next_match() {
        let mut finder = FinderFixture::opened(&FinderFixture::fixture());

        FinderFixture::press(&mut finder, KeyCode::Down, KeyModifiers::NONE);
        FinderFixture::press(&mut finder, KeyCode::Char('j'), KeyModifiers::CONTROL);

        assert_eq!(
            finder.choice(),
            FinderFixture::chosen(FinderFixture::GATEWAY)
        );
    }

    #[test]
    fn up_and_control_k_move_back_and_stop_at_the_first_match() {
        let mut finder = FinderFixture::opened(&FinderFixture::fixture());
        FinderFixture::press(&mut finder, KeyCode::Down, KeyModifiers::NONE);

        FinderFixture::press(&mut finder, KeyCode::Char('k'), KeyModifiers::CONTROL);
        FinderFixture::press(&mut finder, KeyCode::Up, KeyModifiers::NONE);

        assert_eq!(
            finder.choice(),
            FinderFixture::chosen(FinderFixture::RINGLEADER)
        );
    }

    #[test]
    fn a_query_without_matches_offers_the_typed_path() {
        let mut finder = FinderFixture::opened(&FinderFixture::fixture());

        FinderFixture::type_text(&mut finder, "/srv/tools");

        assert!(
            FinderFixture::text(&finder).contains("no repo matches, Enter adds the typed path")
        );
        assert_eq!(
            finder.choice(),
            FinderChoice::TypedPath("/srv/tools".to_string())
        );
    }

    #[test]
    fn an_empty_home_offers_nothing_until_a_path_is_typed() {
        let fixture = ServiceFixture::new().with_scanner(FakeRepoScanner::finding(Vec::new()));
        let finder = FinderFixture::opened(&fixture);

        assert!(FinderFixture::text(&finder).contains("no repos found in /home/example"));
        assert_eq!(finder.choice(), FinderChoice::Nothing);
    }

    #[test]
    fn a_failed_search_shows_its_error() {
        let fixture = ServiceFixture::new().with_scanner(
            FakeRepoScanner::finding(Vec::new()).failing(ScanError::Unreadable {
                path: PathBuf::from("/home/example"),
                detail: "permission denied".to_string(),
            }),
        );

        let finder = FinderFixture::opened(&fixture);

        assert!(
            FinderFixture::text(&finder).contains("cannot list /home/example: permission denied")
        );
    }

    #[test]
    fn a_pasted_query_filters_the_list() {
        let mut finder = FinderFixture::opened(&FinderFixture::fixture());

        finder.paste(" gateway\n").unwrap();

        assert_eq!(
            finder.choice(),
            FinderFixture::chosen(FinderFixture::GATEWAY)
        );
    }

    #[test]
    fn the_list_scrolls_to_keep_the_cursor_in_view() {
        let mut finder = FinderFixture::opened(&FinderFixture::fixture());
        FinderFixture::press(&mut finder, KeyCode::Down, KeyModifiers::NONE);
        FinderFixture::press(&mut finder, KeyCode::Down, KeyModifiers::NONE);

        let shown = finder.lines(60, 3, &Theme);

        assert_eq!(shown.len(), 3);
        assert_eq!(shown[2].spans[0].content, "~/work/api-gateway");
    }
}

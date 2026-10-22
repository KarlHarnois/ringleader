use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard, PoisonError};

use super::gh_program::GhProgram;
use crate::adapters::BoundedCommand;
use crate::adapters::child_process::bounded_command::{Call, CommandError};
use crate::adapters::child_process::command_output::CommandOutput;
use crate::adapters::child_process::deadlines::CallKind;
use crate::application::ports::{RepoPageError, RepoPages};

pub struct GhRepoPages {
    gh: GhProgram,
    bounded: BoundedCommand,
    found: Mutex<HashMap<PathBuf, String>>,
}

impl GhRepoPages {
    pub const BROWSE: &'static str = "browse";

    pub fn new(gh: GhProgram, bounded: BoundedCommand) -> Self {
        Self {
            gh,
            bounded,
            found: Mutex::default(),
        }
    }

    fn remembered(&self, repo: &Path) -> Option<String> {
        self.found_pages().get(repo).cloned()
    }

    fn look_up(&self, repo: &Path) -> Result<Option<String>, RepoPageError> {
        let call = Call::new(Self::BROWSE, CallKind::GhQuery);
        let output = self
            .bounded
            .run(self.browse_command(repo), &call)
            .map_err(Self::error)?;
        Ok(Self::page(repo, &output))
    }

    fn remember(&self, repo: &Path, page: &str) {
        self.found_pages()
            .insert(repo.to_path_buf(), page.to_string());
    }

    fn found_pages(&self) -> MutexGuard<'_, HashMap<PathBuf, String>> {
        self.found.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn browse_command(&self, repo: &Path) -> Command {
        let mut command = self.gh.in_repo(repo);
        command.args([Self::BROWSE, "--no-browser"]);
        command
    }

    fn page(repo: &Path, output: &CommandOutput) -> Option<String> {
        if !output.succeeded() {
            log::debug!(
                "gh knows no GitHub repo for {}: {}",
                repo.display(),
                output.stderr_text()
            );
            return None;
        }
        let page = output.stdout_text().trim().to_string();
        (!page.is_empty()).then_some(page)
    }

    fn error(error: CommandError) -> RepoPageError {
        match error {
            CommandError::TimedOut { seconds, .. } => RepoPageError::TimedOut { seconds },
            CommandError::Spawn { detail, .. } => {
                log::warn!("cannot start gh {}: {detail}", Self::BROWSE);
                RepoPageError::NotStarted
            }
        }
    }
}

impl RepoPages for GhRepoPages {
    fn main_page(&self, repo: &Path) -> Result<Option<String>, RepoPageError> {
        if let Some(page) = self.remembered(repo) {
            return Ok(Some(page));
        }
        let page = self.look_up(repo)?;
        if let Some(page) = &page {
            self.remember(repo, page);
        }
        Ok(page)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::time::Duration;

    use super::*;
    use crate::adapters::Deadlines;
    use crate::test_support::ExecutableScript;

    struct PagesFixture {
        directory: tempfile::TempDir,
    }

    impl PagesFixture {
        const PAGE: &'static str = "https://github.example.com/example-org/example-project";
        const BROWSING_GH: &'static str = r#"#!/bin/sh
echo "$*" >> "$0.calls"
printf 'https://github.example.com/example-org/example-project\n'
"#;
        const LOCAL_ONLY_GH: &'static str = r#"#!/bin/sh
echo "$*" >> "$0.calls"
echo "none of the git remotes configured for this repository point to a known GitHub host" >&2
exit 1
"#;
        const SILENT_GH: &'static str = "#!/bin/sh\nexit 0\n";
        const SLOW_GH: &'static str = "#!/bin/sh\nexec sleep 5\n";
        const SHORT_DEADLINE: Duration = Duration::from_millis(200);

        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn pages(&self, body: &str) -> GhRepoPages {
            self.pages_within(body, Deadlines::standard())
        }

        fn pages_within(&self, body: &str, deadlines: Deadlines) -> GhRepoPages {
            let path = self.directory.path().join("fake-gh");
            ExecutableScript::write(&path, body);
            GhRepoPages::new(
                GhProgram::new(path.as_os_str()),
                BoundedCommand::new(deadlines),
            )
        }

        fn repo(&self) -> &Path {
            self.directory.path()
        }

        fn calls(&self) -> Vec<String> {
            fs::read_to_string(self.directory.path().join("fake-gh.calls"))
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect()
        }
    }

    #[test]
    fn the_main_page_is_the_url_gh_browse_prints() {
        let fixture = PagesFixture::new();
        let pages = fixture.pages(PagesFixture::BROWSING_GH);

        let page = pages.main_page(fixture.repo()).unwrap();

        assert_eq!(page.as_deref(), Some(PagesFixture::PAGE));
        assert_eq!(fixture.calls(), ["browse --no-browser"]);
    }

    #[test]
    fn a_checkout_gh_knows_no_github_repo_for_has_no_main_page() {
        let fixture = PagesFixture::new();
        let pages = fixture.pages(PagesFixture::LOCAL_ONLY_GH);

        assert_eq!(pages.main_page(fixture.repo()), Ok(None));
    }

    #[test]
    fn a_gh_browse_that_prints_no_url_gives_no_main_page() {
        let fixture = PagesFixture::new();
        let pages = fixture.pages(PagesFixture::SILENT_GH);

        assert_eq!(pages.main_page(fixture.repo()), Ok(None));
    }

    #[test]
    fn a_found_main_page_is_looked_up_once() {
        let fixture = PagesFixture::new();
        let pages = fixture.pages(PagesFixture::BROWSING_GH);

        for _ in 0..2 {
            pages.main_page(fixture.repo()).unwrap();
        }

        assert_eq!(fixture.calls().len(), 1);
    }

    #[test]
    fn a_checkout_without_a_main_page_is_looked_up_again_since_it_can_gain_a_remote() {
        let fixture = PagesFixture::new();
        let pages = fixture.pages(PagesFixture::LOCAL_ONLY_GH);

        for _ in 0..2 {
            pages.main_page(fixture.repo()).unwrap();
        }

        assert_eq!(fixture.calls().len(), 2);
    }

    #[test]
    fn a_gh_that_hangs_times_out() {
        let fixture = PagesFixture::new();
        let deadlines = Deadlines::uniform(PagesFixture::SHORT_DEADLINE);
        let pages = fixture.pages_within(PagesFixture::SLOW_GH, deadlines);

        assert!(matches!(
            pages.main_page(fixture.repo()),
            Err(RepoPageError::TimedOut { .. })
        ));
    }

    #[test]
    fn a_gh_that_cannot_start_is_not_started() {
        let fixture = PagesFixture::new();
        let missing = GhProgram::new(OsStr::new("/nonexistent/gh"));
        let pages = GhRepoPages::new(missing, BoundedCommand::new(Deadlines::standard()));

        assert_eq!(
            pages.main_page(fixture.repo()),
            Err(RepoPageError::NotStarted)
        );
    }
}

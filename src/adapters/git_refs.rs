use std::path::Path;

use super::branch_ref::BranchRef;
use super::git_invocation::GitInvocation;
use super::git_runner::GitRunner;
use crate::application::ports::{Head, WorkspaceError};
use crate::domain::{Branch, BranchTips};

pub struct GitRefs<'a> {
    git: &'a GitRunner,
}

impl<'a> GitRefs<'a> {
    const HEAD: &'static str = "HEAD";
    const SYMBOLIC_TARGET_FORMAT: &'static str = "--format=%(refname) %(symref)";
    const TIP_FORMAT: &'static str = "--format=%(objectname) %(refname)";

    pub fn new(git: &'a GitRunner) -> Self {
        Self { git }
    }

    pub fn head(&self, location: &Path) -> Result<Head, WorkspaceError> {
        let verify = GitInvocation::in_worktree(location, "rev-parse")
            .diffing()
            .args(["--verify", "-q", "--symbolic-full-name", Self::HEAD]);
        let checked_out = self.git.resolved_from(&verify)?;
        Ok(checked_out.map_or(Head::Unborn, |name| {
            BranchRef::parse(&name).map_or(Head::Detached, Head::On)
        }))
    }

    pub fn remote_default_branch(
        &self,
        location: &Path,
        remote: &str,
    ) -> Result<Option<String>, WorkspaceError> {
        let remote_branches = format!("{}{remote}/", BranchRef::REMOTE_TRACKING_PREFIX);
        let remote_head = format!("{remote_branches}{}", Self::HEAD);
        let listed = self.listing(
            location,
            Self::SYMBOLIC_TARGET_FORMAT,
            std::slice::from_ref(&remote_head),
        )?;
        Ok(listed
            .into_iter()
            .find(|(name, _)| *name == remote_head)
            .and_then(|(_, target)| target.strip_prefix(&remote_branches).map(str::to_string))
            .filter(|branch| !branch.is_empty()))
    }

    pub fn branch_tips(
        &self,
        location: &Path,
        branches: &[Branch],
    ) -> Result<BranchTips, WorkspaceError> {
        if branches.is_empty() {
            return Ok(BranchTips::default());
        }
        let full_names: Vec<String> = branches
            .iter()
            .map(|branch| BranchRef::new(branch).full_name())
            .collect();
        let listed = self.listing(location, Self::TIP_FORMAT, &full_names)?;
        let tips = branches
            .iter()
            .zip(&full_names)
            .filter_map(|(branch, full_name)| {
                listed
                    .iter()
                    .find(|(_, listed_name)| listed_name == full_name)
                    .map(|(commit, _)| (branch.clone(), commit.clone()))
            });
        Ok(BranchTips::new(tips.collect()))
    }

    fn listing(
        &self,
        location: &Path,
        format: &str,
        patterns: &[String],
    ) -> Result<Vec<(String, String)>, WorkspaceError> {
        let list = GitInvocation::in_worktree(location, "for-each-ref")
            .diffing()
            .arg(format)
            .args(patterns);
        let listing = self.git.run_checked(&list)?;
        Ok(listing
            .lines()
            .into_iter()
            .filter_map(|line| line.split_once(' '))
            .map(|(first, second)| (first.to_string(), second.to_string()))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::GitTestRepo;

    struct RefsFixture {
        repo: GitTestRepo,
        git: GitRunner,
    }

    impl RefsFixture {
        const ORIGIN_HEAD: &'static str = "refs/remotes/origin/HEAD";
        const ORIGIN_MAIN: &'static str = "refs/remotes/origin/main";

        fn new() -> Self {
            Self::around(GitTestRepo::new())
        }

        fn around(repo: GitTestRepo) -> Self {
            let git = repo.runner();
            Self { repo, git }
        }

        fn refs(&self) -> GitRefs<'_> {
            GitRefs::new(&self.git)
        }

        fn run(&self, args: &[&str]) -> String {
            self.repo.git(self.repo.path(), args)
        }

        fn tip(&self) -> String {
            self.run(&["rev-parse", "HEAD"])
        }

        fn head(&self) -> Head {
            self.refs().head(self.repo.path()).unwrap()
        }

        fn origin_default(&self) -> Option<String> {
            self.refs()
                .remote_default_branch(self.repo.path(), Branch::ORIGIN)
                .unwrap()
        }

        fn tips(&self, branches: &[Branch]) -> BranchTips {
            self.refs().branch_tips(self.repo.path(), branches).unwrap()
        }

        fn local(name: &str) -> Branch {
            Branch::Local(name.to_string())
        }
    }

    #[test]
    fn an_unborn_head_is_unborn() {
        let fixture = RefsFixture::around(GitTestRepo::unborn());

        assert_eq!(fixture.head(), Head::Unborn);
    }

    #[test]
    fn a_head_on_a_branch_names_it() {
        let fixture = RefsFixture::new();

        assert_eq!(
            fixture.head(),
            Head::On(RefsFixture::local(GitTestRepo::BRANCH))
        );
    }

    #[test]
    fn a_detached_head_names_no_branch() {
        let fixture = RefsFixture::new();
        fixture.run(&["checkout", "-q", "--detach"]);

        assert_eq!(fixture.head(), Head::Detached);
    }

    #[test]
    fn a_symbolic_origin_head_names_the_default_branch() {
        let fixture = RefsFixture::new();
        fixture.run(&["update-ref", RefsFixture::ORIGIN_MAIN, &fixture.tip()]);
        fixture.run(&[
            "symbolic-ref",
            RefsFixture::ORIGIN_HEAD,
            RefsFixture::ORIGIN_MAIN,
        ]);

        assert_eq!(fixture.origin_default().as_deref(), Some("main"));
    }

    #[test]
    fn a_plain_origin_head_names_no_default_branch() {
        let fixture = RefsFixture::new();
        let tip = fixture.tip();
        fixture.run(&["update-ref", "--no-deref", RefsFixture::ORIGIN_HEAD, &tip]);

        assert_eq!(fixture.origin_default(), None);
    }

    #[test]
    fn an_origin_head_naming_a_deleted_branch_names_no_default_branch() {
        let fixture = RefsFixture::new();
        fixture.run(&[
            "symbolic-ref",
            RefsFixture::ORIGIN_HEAD,
            RefsFixture::ORIGIN_MAIN,
        ]);

        assert_eq!(fixture.origin_default(), None);
    }

    #[test]
    fn a_repo_without_origin_has_no_default_branch() {
        assert_eq!(RefsFixture::new().origin_default(), None);
    }

    #[test]
    fn a_local_branch_resolves_to_its_tip() {
        let fixture = RefsFixture::new();
        let main = RefsFixture::local(GitTestRepo::BRANCH);

        let tips = fixture.tips(std::slice::from_ref(&main));

        assert_eq!(tips.tip_of(&main), Some(fixture.tip().as_str()));
    }

    #[test]
    fn a_remote_tracking_name_resolves_to_its_tip() {
        let fixture = RefsFixture::new();
        fixture.run(&["update-ref", RefsFixture::ORIGIN_MAIN, &fixture.tip()]);
        let origin_main = Branch::origin_copy_of(GitTestRepo::BRANCH);

        let tips = fixture.tips(std::slice::from_ref(&origin_main));

        assert_eq!(tips.tip_of(&origin_main), Some(fixture.tip().as_str()));
    }

    #[test]
    fn a_tag_named_like_a_branch_is_no_branch_tip() {
        let fixture = RefsFixture::new();
        fixture.run(&["tag", "develop"]);
        let develop = RefsFixture::local("develop");

        assert_eq!(
            fixture.tips(std::slice::from_ref(&develop)),
            BranchTips::default()
        );
    }

    #[test]
    fn a_branch_nested_below_the_name_is_no_tip_of_it() {
        let fixture = RefsFixture::new();
        fixture.run(&["branch", "feature/login"]);
        let feature = RefsFixture::local("feature");

        assert_eq!(
            fixture.tips(std::slice::from_ref(&feature)),
            BranchTips::default()
        );
    }

    #[test]
    fn glob_and_revision_syntax_name_no_branch() {
        let fixture = RefsFixture::new();
        let asked = [RefsFixture::local("ma*"), RefsFixture::local("main~1")];

        assert_eq!(fixture.tips(&asked), BranchTips::default());
    }

    #[test]
    fn asking_for_no_branch_finds_no_tip() {
        assert_eq!(RefsFixture::new().tips(&[]), BranchTips::default());
    }
}

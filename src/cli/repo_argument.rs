use super::{CliSession, CommandStop};
use crate::application::{ServiceError, SessionService};
use crate::domain::Repo;

pub struct RepoArgument<'a> {
    value: &'a str,
}

impl<'a> RepoArgument<'a> {
    pub fn new(value: &'a str) -> Self {
        Self { value }
    }

    pub fn resolve(
        &self,
        service: &SessionService,
        session: &CliSession,
    ) -> Result<Repo, CommandStop> {
        let discovery = session.wait(&service.discover_repos(service.launch_directory()))?;
        if !self.is_path() {
            let matches: Vec<&Repo> = discovery
                .repos()
                .iter()
                .filter(|repo| repo.name().as_str() == self.value)
                .collect();
            match matches.as_slice() {
                [repo] => return Ok((*repo).clone()),
                [] => {}
                several => return Err(self.ambiguous(several).into()),
            }
        }
        session.wait(&service.repo_at(self.value))
    }

    fn is_path(&self) -> bool {
        matches!(self.value, "." | "..") || self.value.starts_with('~') || self.value.contains('/')
    }

    fn ambiguous(&self, matches: &[&Repo]) -> ServiceError {
        let paths: Vec<String> = matches
            .iter()
            .map(|repo| repo.path().display().to_string())
            .collect();
        ServiceError::Invalid(format!(
            "repo {} matches several repos, pass one of these paths instead: {}",
            self.value,
            paths.join(", ")
        ))
    }
}

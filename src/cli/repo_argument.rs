use super::{CliSession, CommandStop};
use crate::application::{ServiceError, SessionService};
use crate::domain::{Repo, RepoName};

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
        let typed = session.wait(&service.read_typed_path(self.value))?;
        let top_level = session.wait(&service.validated_top_level(&typed))?;
        let configured = discovery
            .repos()
            .iter()
            .find(|repo| repo.path() == top_level);
        match configured {
            Some(repo) => Ok(repo.clone()),
            None => Self::discovered(&top_level),
        }
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

    fn discovered(top_level: &std::path::Path) -> Result<Repo, CommandStop> {
        let basename = top_level
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = RepoName::from_basename(&basename).map_err(ServiceError::from)?;
        Ok(Repo::discovered(name, top_level))
    }
}

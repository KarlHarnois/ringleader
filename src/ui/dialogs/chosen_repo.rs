use super::home_path::HomePath;
use crate::application::{Pending, SessionService};
use crate::domain::Repo;

pub struct ChosenRepo {
    state: ChoiceState,
    home: HomePath,
}

impl ChosenRepo {
    const FINDING: &'static str = "Finding the repo you started in…";
    const NONE: &'static str = "none, type to find one";

    pub fn open(service: &SessionService) -> Self {
        Self {
            state: ChoiceState::Finding(service.default_repo()),
            home: HomePath::new(service.home_directory()),
        }
    }

    pub fn poll(&mut self) {
        if let ChoiceState::Finding(pending) = &self.state
            && let Some(result) = pending.poll()
        {
            self.state = ChoiceState::Ready(result.ok().flatten());
        }
    }

    pub fn set(&mut self, repo: Repo) {
        self.state = ChoiceState::Ready(Some(repo));
    }

    pub fn repo(&self) -> Option<&Repo> {
        match &self.state {
            ChoiceState::Ready(repo) => repo.as_ref(),
            ChoiceState::Finding(_) => None,
        }
    }

    pub fn text(&self) -> String {
        match &self.state {
            ChoiceState::Finding(_) => Self::FINDING.to_string(),
            ChoiceState::Ready(Some(repo)) => {
                format!("{} · {}", repo.name(), self.home.shown(repo.path()))
            }
            ChoiceState::Ready(None) => Self::NONE.to_string(),
        }
    }
}

enum ChoiceState {
    Finding(Pending<Option<Repo>>),
    Ready(Option<Repo>),
}

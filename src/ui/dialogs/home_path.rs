use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct HomePath {
    home: Option<PathBuf>,
}

impl HomePath {
    pub fn new(home: Option<&Path>) -> Self {
        Self {
            home: home.map(Path::to_path_buf),
        }
    }

    pub fn shown(&self, path: &Path) -> String {
        let under_home = self
            .home
            .as_deref()
            .and_then(|home| path.strip_prefix(home).ok());
        match under_home {
            Some(rest) => format!("~/{}", rest.display()),
            None => path.display().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct HomeFixture;

    impl HomeFixture {
        fn home() -> HomePath {
            HomePath::new(Some(Path::new("/home/example")))
        }
    }

    #[test]
    fn a_path_under_home_is_shown_from_the_tilde() {
        let shown = HomeFixture::home().shown(Path::new("/home/example/hobby/ringleader"));

        assert_eq!(shown, "~/hobby/ringleader");
    }

    #[test]
    fn a_path_outside_home_stays_absolute() {
        let shown = HomeFixture::home().shown(Path::new("/srv/shared/tools"));

        assert_eq!(shown, "/srv/shared/tools");
    }

    #[test]
    fn without_a_home_every_path_stays_absolute() {
        let shown = HomePath::default().shown(Path::new("/home/example/notes"));

        assert_eq!(shown, "/home/example/notes");
    }
}

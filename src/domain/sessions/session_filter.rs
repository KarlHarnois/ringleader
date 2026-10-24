use super::session_summary::SessionSummary;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SessionFilter {
    #[default]
    All,
    Favorites,
}

impl SessionFilter {
    pub fn toggled(self) -> Self {
        match self {
            Self::All => Self::Favorites,
            Self::Favorites => Self::All,
        }
    }

    pub fn allows(self, row: &SessionSummary) -> bool {
        match self {
            Self::All => true,
            Self::Favorites => row.is_favorite(),
        }
    }

    pub fn shows_only_favorites(self) -> bool {
        self == Self::Favorites
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::SessionState;
    use crate::test_support::ServiceFixture;

    struct FilterFixture;

    impl FilterFixture {
        fn favorite() -> SessionSummary {
            let session = ServiceFixture::session("fix-auth").favorited();
            SessionSummary::managed(session, SessionState::Waiting, false)
        }

        fn plain() -> SessionSummary {
            SessionSummary::managed(
                ServiceFixture::session("fix-auth"),
                SessionState::Waiting,
                false,
            )
        }
    }

    #[test]
    fn the_default_filter_allows_every_row() {
        assert!(SessionFilter::default().allows(&FilterFixture::plain()));
        assert!(SessionFilter::default().allows(&FilterFixture::favorite()));
    }

    #[test]
    fn the_favorites_filter_allows_only_a_favorite() {
        assert!(SessionFilter::Favorites.allows(&FilterFixture::favorite()));
        assert!(!SessionFilter::Favorites.allows(&FilterFixture::plain()));
    }

    #[test]
    fn toggling_switches_between_the_two_filters() {
        assert_eq!(SessionFilter::All.toggled(), SessionFilter::Favorites);
        assert_eq!(SessionFilter::Favorites.toggled(), SessionFilter::All);
    }

    #[test]
    fn only_the_favorites_filter_shows_only_favorites() {
        assert!(!SessionFilter::All.shows_only_favorites());
        assert!(SessionFilter::Favorites.shows_only_favorites());
    }
}

use ratatui::text::Line;

use crate::application::{Pending, SessionService};
use crate::domain::Repo;
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

pub struct TypedRepoCheck {
    state: CheckState,
}

impl TypedRepoCheck {
    pub fn start(service: &SessionService, typed: &str) -> Self {
        Self {
            state: CheckState::Checking {
                typed: typed.to_string(),
                pending: service.repo_at(typed),
            },
        }
    }

    pub fn poll(&mut self) -> Option<Repo> {
        let CheckState::Checking { pending, .. } = &self.state else {
            return None;
        };
        let result = pending.poll()?;
        self.state = match &result {
            Ok(_) => CheckState::Confirmed,
            Err(error) => CheckState::Refused(error.to_string()),
        };
        result.ok()
    }

    pub fn lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        match &self.state {
            CheckState::Checking { typed, .. } => {
                let checking = format!("Checking {typed}…");
                TextFit::new(&checking).styled_lines(width, theme.notice())
            }
            CheckState::Refused(error) => TextFit::new(error).styled_lines(width, theme.error()),
            CheckState::Confirmed => Vec::new(),
        }
    }
}

enum CheckState {
    Checking {
        typed: String,
        pending: Pending<Repo>,
    },
    Refused(String),
    Confirmed,
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::test_support::{FakeHostProbe, ServiceFixture};

    struct CheckFixture;

    impl CheckFixture {
        const SETTLE_LIMIT: Duration = Duration::from_secs(2);
        const TYPED: &'static str = "/srv/tools";

        fn confirmed() -> TypedRepoCheck {
            let host = FakeHostProbe::default().with_directory(Path::new(Self::TYPED));
            let service = ServiceFixture::new().with_host(host).service();
            let mut check = TypedRepoCheck::start(&service, Self::TYPED);
            let deadline = Instant::now() + Self::SETTLE_LIMIT;
            while check.poll().is_none() {
                assert!(Instant::now() < deadline, "the check confirmed in time");
                thread::sleep(Duration::from_millis(5));
            }
            check
        }
    }

    #[test]
    fn a_confirmed_check_stays_quiet_when_polled_again() {
        let mut check = CheckFixture::confirmed();

        let again = check.poll();

        assert_eq!(again, None);
        assert!(check.lines(60, &Theme::default()).is_empty());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimitWindow {
    Session,
    Week,
    Scoped(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageLimit {
    window: LimitWindow,
    used_percent: u8,
    resets_at: Option<u64>,
}

impl UsageLimit {
    const FULL_PERCENT: f64 = 100.0;

    pub fn used(window: LimitWindow, percent: f64, resets_at: Option<u64>) -> Self {
        Self {
            window,
            used_percent: percent.round().clamp(0.0, Self::FULL_PERCENT) as u8,
            resets_at,
        }
    }

    pub fn window(&self) -> &LimitWindow {
        &self.window
    }

    pub fn used_percent(&self) -> u8 {
        self.used_percent
    }

    pub fn resets_at(&self) -> Option<u64> {
        self.resets_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_percent_above_a_hundred_is_capped() {
        let limit = UsageLimit::used(LimitWindow::Session, 130.0, None);

        assert_eq!(limit.used_percent(), 100);
    }

    #[test]
    fn a_negative_percent_is_zero() {
        let limit = UsageLimit::used(LimitWindow::Session, -4.0, None);

        assert_eq!(limit.used_percent(), 0);
    }

    #[test]
    fn a_fractional_percent_rounds_to_the_nearest_whole() {
        let limit = UsageLimit::used(LimitWindow::Week, 41.5, None);

        assert_eq!(limit.used_percent(), 42);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyHints {
    hints: &'static [(&'static str, &'static str)],
    drop_order: &'static [&'static str],
}

impl KeyHints {
    const SESSION_HINTS: [(&'static str, &'static str); 10] = [
        ("n", "new"),
        ("d", "delete"),
        ("Enter", "attach"),
        ("/", "find"),
        ("t", "shell"),
        ("o", "PR"),
        ("p", "pull"),
        ("u", "usage"),
        ("?", "help"),
        ("q", "quit"),
    ];
    const SESSION_DROP_ORDER: [&'static str; 6] = ["u", "p", "/", "t", "o", "d"];
    const HEADING_HINTS: [(&'static str, &'static str); 6] = [
        ("n", "new"),
        ("Enter", "fold"),
        ("/", "find"),
        ("u", "usage"),
        ("?", "help"),
        ("q", "quit"),
    ];
    const HEADING_DROP_ORDER: [&'static str; 2] = ["u", "/"];
    const NO_SELECTION_HINTS: [(&'static str, &'static str); 4] =
        [("n", "new"), ("u", "usage"), ("?", "help"), ("q", "quit")];
    const WIDE_GAP: &'static str = "   ";
    const NARROW_GAP: &'static str = "  ";

    pub fn on_session() -> Self {
        Self {
            hints: &Self::SESSION_HINTS,
            drop_order: &Self::SESSION_DROP_ORDER,
        }
    }

    pub fn on_heading() -> Self {
        Self {
            hints: &Self::HEADING_HINTS,
            drop_order: &Self::HEADING_DROP_ORDER,
        }
    }

    pub fn without_selection() -> Self {
        Self {
            hints: &Self::NO_SELECTION_HINTS,
            drop_order: &[],
        }
    }

    pub fn text(&self, width: usize) -> String {
        let shown = self.fitted(width);
        let gap = if Self::width_of(&shown, Self::WIDE_GAP) <= width {
            Self::WIDE_GAP
        } else {
            Self::NARROW_GAP
        };
        Self::joined(&shown, gap)
    }

    fn fitted(&self, width: usize) -> Vec<String> {
        let mut shown: Vec<String> = self
            .hints
            .iter()
            .map(|(key, action)| format!("{key} {action}"))
            .collect();
        for dropped in self.drop_order {
            if Self::width_of(&shown, Self::WIDE_GAP) <= width {
                break;
            }
            shown.retain(|hint| hint.split(' ').next() != Some(*dropped));
        }
        shown
    }

    fn width_of(hints: &[String], gap: &str) -> usize {
        Self::joined(hints, gap).chars().count()
    }

    fn joined(hints: &[String], gap: &str) -> String {
        format!(" {}", hints.join(gap))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ninety_five_column_bar_lists_every_session_hint() {
        assert_eq!(
            KeyHints::on_session().text(95),
            " n new   d delete   Enter attach   / find   t shell   o PR   p pull   u usage   ? help   q quit"
        );
    }

    #[test]
    fn an_eighty_column_bar_drops_usage_and_pull() {
        assert_eq!(
            KeyHints::on_session().text(80),
            " n new   d delete   Enter attach   / find   t shell   o PR   ? help   q quit"
        );
    }

    #[test]
    fn a_sixty_column_bar_also_drops_find_and_shell() {
        assert_eq!(
            KeyHints::on_session().text(60),
            " n new   d delete   Enter attach   o PR   ? help   q quit"
        );
    }

    #[test]
    fn a_thirty_six_column_bar_keeps_the_fixed_keys_with_narrower_gaps() {
        assert_eq!(
            KeyHints::on_session().text(36),
            " n new  Enter attach  ? help  q quit"
        );
    }

    #[test]
    fn a_heading_bar_offers_enter_fold_and_no_session_key() {
        assert_eq!(
            KeyHints::on_heading().text(95),
            " n new   Enter fold   / find   u usage   ? help   q quit"
        );
    }

    #[test]
    fn a_fifty_column_heading_bar_drops_usage_first() {
        assert_eq!(
            KeyHints::on_heading().text(50),
            " n new   Enter fold   / find   ? help   q quit"
        );
    }

    #[test]
    fn a_forty_column_heading_bar_also_drops_find() {
        assert_eq!(
            KeyHints::on_heading().text(40),
            " n new   Enter fold   ? help   q quit"
        );
    }

    #[test]
    fn a_bar_without_a_selection_lists_only_keys_that_need_no_session() {
        assert_eq!(
            KeyHints::without_selection().text(95),
            " n new   u usage   ? help   q quit"
        );
    }
}

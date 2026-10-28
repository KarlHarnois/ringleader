use ratatui::text::{Line, Span};

use crate::ui::theme::Theme;

type Hint = (&'static str, &'static str);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyHints {
    hints: &'static [Hint],
}

impl KeyHints {
    const NEW: Hint = ("n", "new");
    const DELETE: Hint = ("d", "delete");
    const ATTACH: Hint = ("Enter", "attach");
    const FOLD: Hint = ("Enter", "fold");
    const FIND: Hint = ("/", "find");
    const SHELL: Hint = ("t", "shell");
    const PULL_REQUEST: Hint = ("o", "PR");
    const PULL: Hint = ("p", "pull");
    const USAGE: Hint = ("u", "usage");
    const HELP: Hint = ("?", "help");
    const QUIT: Hint = ("q", "quit");
    const SESSION_HINTS: [Hint; 10] = [
        Self::NEW,
        Self::DELETE,
        Self::ATTACH,
        Self::FIND,
        Self::SHELL,
        Self::PULL_REQUEST,
        Self::PULL,
        Self::USAGE,
        Self::HELP,
        Self::QUIT,
    ];
    const HEADING_HINTS: [Hint; 6] = [
        Self::NEW,
        Self::FOLD,
        Self::FIND,
        Self::USAGE,
        Self::HELP,
        Self::QUIT,
    ];
    const NO_SELECTION_HINTS: [Hint; 4] = [Self::NEW, Self::USAGE, Self::HELP, Self::QUIT];
    const DROP_ORDER: [Hint; 6] = [
        Self::USAGE,
        Self::PULL,
        Self::FIND,
        Self::SHELL,
        Self::PULL_REQUEST,
        Self::DELETE,
    ];
    const WIDE_GAP: &'static str = "   ";
    const NARROW_GAP: &'static str = "  ";
    const MARGIN: &'static str = " ";

    pub fn on_session() -> Self {
        Self {
            hints: &Self::SESSION_HINTS,
        }
    }

    pub fn on_heading() -> Self {
        Self {
            hints: &Self::HEADING_HINTS,
        }
    }

    pub fn without_selection() -> Self {
        Self {
            hints: &Self::NO_SELECTION_HINTS,
        }
    }

    pub fn line(&self, width: usize, theme: &Theme) -> Line<'static> {
        let (shown, gap) = self.layout(width);
        let spans: Vec<Span<'static>> = shown
            .iter()
            .enumerate()
            .flat_map(|(index, (key, action))| {
                let lead = if index == 0 { Self::MARGIN } else { gap };
                [
                    Span::styled(lead, theme.hint_bar_text()),
                    Span::styled(*key, theme.hint_bar_key()),
                    Span::styled(format!(" {action}"), theme.hint_bar_text()),
                ]
            })
            .collect();
        Line::from(spans)
    }

    #[cfg(test)]
    pub fn text(&self, width: usize) -> String {
        let (shown, gap) = self.layout(width);
        Self::joined(&shown, gap)
    }

    fn layout(&self, width: usize) -> (Vec<Hint>, &'static str) {
        let shown = self.fitted(width);
        let gap = if Self::width_of(&shown, Self::WIDE_GAP) <= width {
            Self::WIDE_GAP
        } else {
            Self::NARROW_GAP
        };
        (shown, gap)
    }

    fn fitted(&self, width: usize) -> Vec<Hint> {
        let mut shown = self.hints.to_vec();
        for dropped in Self::DROP_ORDER {
            if Self::width_of(&shown, Self::WIDE_GAP) <= width {
                break;
            }
            shown.retain(|hint| *hint != dropped);
        }
        shown
    }

    fn width_of(hints: &[Hint], gap: &str) -> usize {
        Self::joined(hints, gap).chars().count()
    }

    fn joined(hints: &[Hint], gap: &str) -> String {
        let labels: Vec<String> = hints
            .iter()
            .map(|(key, action)| format!("{key} {action}"))
            .collect();
        format!("{}{}", Self::MARGIN, labels.join(gap))
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
    fn a_thirty_six_column_bar_keeps_help_and_quit_so_every_key_stays_reachable() {
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

    #[test]
    fn the_bar_styles_keys_apart_from_their_actions() {
        let theme = Theme::default();

        let line = KeyHints::without_selection().line(95, &theme);

        assert_eq!(
            (line.spans[1].content.as_ref(), line.spans[1].style),
            ("n", theme.hint_bar_key())
        );
    }

    #[test]
    fn the_styled_bar_reads_like_the_plain_one() {
        let line = KeyHints::on_session().line(80, &Theme::default());

        assert_eq!(line.to_string(), KeyHints::on_session().text(80));
    }
}

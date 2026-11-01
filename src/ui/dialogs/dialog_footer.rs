use ratatui::text::Line;

use crate::ui::theme::Theme;

pub struct DialogFooter<'a> {
    status: &'a str,
    hint: &'a str,
}

impl<'a> DialogFooter<'a> {
    const MINIMUM_GAP: usize = 2;

    pub fn new(status: &'a str, hint: &'a str) -> Self {
        Self { status, hint }
    }

    pub fn line(&self, width: usize, theme: &Theme) -> Line<'static> {
        let used = self.status.chars().count() + self.hint.chars().count();
        let gap = width.saturating_sub(used).max(Self::MINIMUM_GAP);
        let footer = format!("{}{}{}", self.status, " ".repeat(gap), self.hint);
        Line::styled(footer, theme.hint())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FooterFixture;

    impl FooterFixture {
        fn text(status: &str, hint: &str, width: usize) -> String {
            DialogFooter::new(status, hint)
                .line(width, &Theme::default())
                .to_string()
        }
    }

    #[test]
    fn the_hint_ends_at_the_right_edge() {
        assert_eq!(
            FooterFixture::text("updated", "q close", 20),
            "updated      q close"
        );
    }

    #[test]
    fn a_narrow_footer_still_keeps_the_status_apart_from_the_hint() {
        assert_eq!(
            FooterFixture::text("updated", "q close", 10),
            "updated  q close"
        );
    }

    #[test]
    fn the_footer_takes_the_hint_style() {
        let theme = Theme::default();

        let line = DialogFooter::new("updated", "q close").line(20, &theme);

        assert_eq!(line.style, theme.hint());
    }
}

use ratatui::style::Style;
use ratatui::text::Line;

pub struct TextFit<'a> {
    text: &'a str,
}

impl<'a> TextFit<'a> {
    const ELLIPSIS: char = '…';

    pub fn new(text: &'a str) -> Self {
        Self { text }
    }

    pub fn padded(&self, width: usize) -> String {
        format!("{:<width$}", self.right_truncated(width))
    }

    pub fn right_truncated(&self, width: usize) -> String {
        if self.length() <= width {
            return self.text.to_string();
        }
        let kept: String = self.text.chars().take(width.saturating_sub(1)).collect();
        format!("{kept}{}", Self::ELLIPSIS)
    }

    pub fn left_truncated(&self, width: usize) -> String {
        if self.length() <= width {
            return self.text.to_string();
        }
        let skipped = self.length() + 1 - width.max(1);
        let kept: String = self.text.chars().skip(skipped).collect();
        format!("{}{kept}", Self::ELLIPSIS)
    }

    pub fn styled_lines(&self, width: usize, style: Style) -> Vec<Line<'static>> {
        let wrapped = self.wrapped(width).into_iter();
        wrapped.map(|line| Line::styled(line, style)).collect()
    }

    pub fn wrapped(&self, width: usize) -> Vec<String> {
        let width = width.max(1);
        let mut lines: Vec<String> = Vec::new();
        let mut current = String::new();
        for word in self.text.split(' ') {
            let needed =
                current.chars().count() + usize::from(!current.is_empty()) + word.chars().count();
            if needed > width && !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
            while current.chars().count() > width {
                let rest: String = current.chars().skip(width).collect();
                lines.push(current.chars().take(width).collect());
                current = rest;
            }
        }
        lines.push(current);
        lines
    }

    fn length(&self) -> usize {
        self.text.chars().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_text_is_padded_to_the_width() {
        assert_eq!(TextFit::new("docs").padded(6), "docs  ");
    }

    #[test]
    fn a_long_text_keeps_its_start_and_ends_with_an_ellipsis() {
        assert_eq!(
            TextFit::new("refactor-the-reconciler").right_truncated(8),
            "refacto…"
        );
    }

    #[test]
    fn a_long_message_wraps_at_word_boundaries() {
        assert_eq!(
            TextFit::new("/work/fix-auth is a linked worktree, add its main repository")
                .wrapped(30),
            [
                "/work/fix-auth is a linked",
                "worktree, add its main",
                "repository"
            ]
        );
    }

    #[test]
    fn a_word_longer_than_the_width_is_split() {
        assert_eq!(
            TextFit::new("abcdefghij end").wrapped(4),
            ["abcd", "efgh", "ij", "end"]
        );
    }

    #[test]
    fn a_long_path_keeps_its_end_and_starts_with_an_ellipsis() {
        assert_eq!(
            TextFit::new("/data/ringleader/worktrees/fix-auth").left_truncated(12),
            "…es/fix-auth"
        );
    }
}

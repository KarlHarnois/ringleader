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
    fn a_long_path_keeps_its_end_and_starts_with_an_ellipsis() {
        assert_eq!(
            TextFit::new("/data/ringleader/worktrees/fix-auth").left_truncated(12),
            "…es/fix-auth"
        );
    }
}

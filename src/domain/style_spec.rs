use super::theme_color::ThemeColor;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StyleSpec {
    foreground: Option<ThemeColor>,
    background: Option<ThemeColor>,
    modifiers: Vec<TextModifier>,
    removed_modifiers: Vec<TextModifier>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextModifier {
    Bold,
    Dim,
    Italic,
    Underlined,
    Reversed,
    CrossedOut,
}

impl StyleSpec {
    pub fn with_foreground(self, foreground: ThemeColor) -> Self {
        Self {
            foreground: Some(foreground),
            ..self
        }
    }

    pub fn with_background(self, background: ThemeColor) -> Self {
        Self {
            background: Some(background),
            ..self
        }
    }

    pub fn with(mut self, modifier: TextModifier) -> Self {
        self.removed_modifiers
            .retain(|removed| *removed != modifier);
        if !self.modifiers.contains(&modifier) {
            self.modifiers.push(modifier);
        }
        self
    }

    pub fn without(mut self, modifier: TextModifier) -> Self {
        self.modifiers.retain(|added| *added != modifier);
        if !self.removed_modifiers.contains(&modifier) {
            self.removed_modifiers.push(modifier);
        }
        self
    }

    pub fn foreground(&self) -> Option<ThemeColor> {
        self.foreground
    }

    pub fn background(&self) -> Option<ThemeColor> {
        self.background
    }

    pub fn modifiers(&self) -> &[TextModifier] {
        &self.modifiers
    }

    pub fn removed_modifiers(&self) -> &[TextModifier] {
        &self.removed_modifiers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_modifier_added_twice_counts_once() {
        let style = StyleSpec::default()
            .with(TextModifier::Bold)
            .with(TextModifier::Bold);

        assert_eq!(style.modifiers(), [TextModifier::Bold]);
    }

    #[test]
    fn removing_a_modifier_undoes_adding_it() {
        let style = StyleSpec::default()
            .with(TextModifier::Bold)
            .without(TextModifier::Bold);

        assert_eq!(
            (style.modifiers(), style.removed_modifiers()),
            (&[][..], &[TextModifier::Bold][..])
        );
    }
}

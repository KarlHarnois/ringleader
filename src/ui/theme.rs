use std::ffi::OsStr;

use ratatui::style::{Color, Modifier, Style};

use crate::domain::{
    AnsiColor, PullRequestState, SessionState, StyleSpec, TextModifier, ThemeColor, ThemeRole,
    ThemeSpec,
};

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    styles: [Style; ThemeRole::ALL.len()],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorMode {
    #[default]
    Full,
    Monochrome,
}

impl Default for Theme {
    fn default() -> Self {
        Self::new(&ThemeSpec::default())
    }
}

impl Theme {
    const ANSI_COLORS: [(AnsiColor, Color); 16] = [
        (AnsiColor::Black, Color::Black),
        (AnsiColor::Red, Color::Red),
        (AnsiColor::Green, Color::Green),
        (AnsiColor::Yellow, Color::Yellow),
        (AnsiColor::Blue, Color::Blue),
        (AnsiColor::Magenta, Color::Magenta),
        (AnsiColor::Cyan, Color::Cyan),
        (AnsiColor::White, Color::Gray),
        (AnsiColor::BrightBlack, Color::DarkGray),
        (AnsiColor::BrightRed, Color::LightRed),
        (AnsiColor::BrightGreen, Color::LightGreen),
        (AnsiColor::BrightYellow, Color::LightYellow),
        (AnsiColor::BrightBlue, Color::LightBlue),
        (AnsiColor::BrightMagenta, Color::LightMagenta),
        (AnsiColor::BrightCyan, Color::LightCyan),
        (AnsiColor::BrightWhite, Color::White),
    ];
    const MODIFIERS: [(TextModifier, Modifier); 6] = [
        (TextModifier::Bold, Modifier::BOLD),
        (TextModifier::Dim, Modifier::DIM),
        (TextModifier::Italic, Modifier::ITALIC),
        (TextModifier::Underlined, Modifier::UNDERLINED),
        (TextModifier::Reversed, Modifier::REVERSED),
        (TextModifier::CrossedOut, Modifier::CROSSED_OUT),
    ];

    pub fn new(spec: &ThemeSpec) -> Self {
        let base = Self::style_of(&spec.style(ThemeRole::Base));
        Self {
            styles: ThemeRole::ALL.map(|role| base.patch(Self::style_of(&spec.style(role)))),
        }
    }

    fn without_colors(self) -> Self {
        Self {
            styles: self.styles.map(|style| Style {
                fg: None,
                bg: None,
                ..style
            }),
        }
    }

    pub fn base(&self) -> Style {
        self.style(ThemeRole::Base)
    }

    pub fn selected(&self) -> Style {
        self.style(ThemeRole::Selected)
    }

    pub fn selected_out_of_focus(&self) -> Style {
        self.style(ThemeRole::SelectedOutOfFocus)
    }

    pub fn on_selection(&self, mut style: Style) -> Style {
        let (base, dim) = (self.base(), self.dim());
        style
            .add_modifier
            .remove(dim.add_modifier.difference(base.add_modifier));
        if style.fg == dim.fg {
            style.fg = base.fg;
        }
        style
    }

    pub fn selected_text(&self) -> Style {
        self.style(ThemeRole::SelectedText)
    }

    pub fn accent(&self) -> Style {
        self.style(ThemeRole::Accent)
    }

    pub fn dim(&self) -> Style {
        self.style(ThemeRole::Dim)
    }

    pub fn emphasis(&self) -> Style {
        self.style(ThemeRole::Emphasis)
    }

    pub fn matched(&self) -> Style {
        self.style(ThemeRole::Matched)
    }

    pub fn error(&self) -> Style {
        self.style(ThemeRole::Error)
    }

    pub fn warning(&self) -> Style {
        self.style(ThemeRole::Warning)
    }

    pub fn insertions(&self) -> Style {
        self.style(ThemeRole::Insertions)
    }

    pub fn deletions(&self) -> Style {
        self.style(ThemeRole::Deletions)
    }

    pub fn border(&self) -> Style {
        self.style(ThemeRole::Border)
    }

    pub fn title(&self) -> Style {
        self.style(ThemeRole::Title)
    }

    pub fn dialog_border(&self) -> Style {
        self.style(ThemeRole::DialogBorder)
    }

    pub fn separator(&self) -> Style {
        self.style(ThemeRole::Separator)
    }

    pub fn table_header(&self) -> Style {
        self.style(ThemeRole::TableHeader)
    }

    pub fn repo_name(&self) -> Style {
        self.style(ThemeRole::RepoName)
    }

    pub fn branch_prefix(&self) -> Style {
        self.style(ThemeRole::BranchPrefix)
    }

    pub fn agent(&self) -> Style {
        self.style(ThemeRole::Agent)
    }

    pub fn hint(&self) -> Style {
        self.style(ThemeRole::Hint)
    }

    pub fn hint_bar_key(&self) -> Style {
        self.style(ThemeRole::HintBarKey)
    }

    pub fn hint_bar_text(&self) -> Style {
        self.style(ThemeRole::HintBarText)
    }

    pub fn status_error(&self) -> Style {
        self.style(ThemeRole::StatusError)
    }

    pub fn state(&self, state: SessionState) -> Style {
        self.style(ThemeRole::State(state))
    }

    pub fn pull_request(&self, state: PullRequestState) -> Style {
        self.style(ThemeRole::PullRequest(state))
    }

    pub fn approved(&self) -> Style {
        self.style(ThemeRole::Approved)
    }

    fn style(&self, role: ThemeRole) -> Style {
        ThemeRole::ALL
            .iter()
            .position(|listed| *listed == role)
            .map_or_else(Style::default, |index| self.styles[index])
    }

    fn style_of(spec: &StyleSpec) -> Style {
        let style = Style::default()
            .add_modifier(Self::modifiers_in(spec.modifiers()))
            .remove_modifier(Self::modifiers_in(spec.removed_modifiers()));
        let colored = match spec.foreground() {
            Some(color) => style.fg(Self::color_of(color)),
            None => style,
        };
        match spec.background() {
            Some(color) => colored.bg(Self::color_of(color)),
            None => colored,
        }
    }

    fn modifiers_in(listed: &[TextModifier]) -> Modifier {
        Self::MODIFIERS
            .iter()
            .filter(|(modifier, _)| listed.contains(modifier))
            .fold(Modifier::empty(), |all, (_, modifier)| all | *modifier)
    }

    fn color_of(color: ThemeColor) -> Color {
        match color {
            ThemeColor::Default => Color::Reset,
            ThemeColor::Ansi(ansi) => Self::ansi_color(ansi),
            ThemeColor::Indexed(index) => Color::Indexed(index),
            ThemeColor::Rgb(red, green, blue) => Color::Rgb(red, green, blue),
        }
    }

    fn ansi_color(ansi: AnsiColor) -> Color {
        Self::ANSI_COLORS
            .iter()
            .find(|(listed, _)| *listed == ansi)
            .map_or(Color::Reset, |(_, color)| *color)
    }
}

impl ColorMode {
    pub const VARIABLE: &'static str = "NO_COLOR";

    pub fn from_no_color(value: Option<&OsStr>) -> Self {
        match value {
            Some(value) if !value.is_empty() => Self::Monochrome,
            _ => Self::Full,
        }
    }

    pub fn theme(self, spec: &ThemeSpec) -> Theme {
        match self {
            Self::Full => Theme::new(spec),
            Self::Monochrome => Theme::new(spec).without_colors(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ThemeOverrides;

    struct ThemeFixture;

    impl ThemeFixture {
        fn with(role: ThemeRole, style: StyleSpec) -> Theme {
            Theme::new(&ThemeSpec::new(ThemeOverrides::default().with(role, style)))
        }
    }

    #[test]
    fn the_default_theme_leaves_every_color_to_the_terminal() {
        let theme = Theme::default();

        assert_eq!(
            (theme.base(), theme.selected(), theme.error()),
            (
                Style::default().fg(Color::Reset).bg(Color::Reset),
                theme.base().add_modifier(Modifier::REVERSED),
                theme.base().fg(Color::Red),
            )
        );
    }

    #[test]
    fn a_hex_color_becomes_an_exact_terminal_color() {
        let accent = StyleSpec::default().with_foreground(ThemeColor::Rgb(0x8a, 0x8a, 0x8d));

        let theme = ThemeFixture::with(ThemeRole::Accent, accent);

        assert_eq!(theme.accent().fg, Some(Color::Rgb(0x8a, 0x8a, 0x8d)));
    }

    #[test]
    fn a_configured_base_background_reaches_every_role() {
        let base = StyleSpec::default().with_background(ThemeColor::Indexed(234));

        let theme = ThemeFixture::with(ThemeRole::Base, base);

        assert_eq!(theme.error().bg, Some(Color::Indexed(234)));
    }

    #[test]
    fn white_maps_to_the_terminals_normal_white() {
        let accent = StyleSpec::default().with_foreground(ThemeColor::Ansi(AnsiColor::White));

        let theme = ThemeFixture::with(ThemeRole::Accent, accent);

        assert_eq!(theme.accent().fg, Some(Color::Gray));
    }

    #[test]
    fn the_selected_line_lifts_a_dim_color() {
        let dim = StyleSpec::default().with_foreground(ThemeColor::Indexed(240));
        let theme = ThemeFixture::with(ThemeRole::Dim, dim);

        assert_eq!(theme.on_selection(theme.dim()).fg, theme.base().fg);
    }

    #[test]
    fn the_selected_line_lifts_the_dim_modifier() {
        let theme = Theme::default();

        assert!(
            !theme
                .on_selection(theme.dim())
                .add_modifier
                .contains(Modifier::DIM)
        );
    }

    #[test]
    fn a_set_no_color_turns_colors_off() {
        assert_eq!(
            ColorMode::from_no_color(Some(OsStr::new("1"))),
            ColorMode::Monochrome
        );
    }

    #[test]
    fn an_empty_no_color_keeps_colors_on() {
        assert_eq!(
            ColorMode::from_no_color(Some(OsStr::new(""))),
            ColorMode::Full
        );
    }

    #[test]
    fn a_monochrome_theme_drops_every_color() {
        let theme = ColorMode::Monochrome.theme(&ThemeSpec::default());

        assert_eq!((theme.error().fg, theme.base().bg), (None, None));
    }

    #[test]
    fn a_monochrome_theme_keeps_the_modifiers() {
        let theme = ColorMode::Monochrome.theme(&ThemeSpec::default());

        assert!(theme.selected().add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn a_role_can_turn_off_a_modifier_set_on_base() {
        let spec = ThemeSpec::new(
            ThemeOverrides::default()
                .with(
                    ThemeRole::Base,
                    StyleSpec::default().with(TextModifier::Bold),
                )
                .with(
                    ThemeRole::Hint,
                    StyleSpec::default().without(TextModifier::Bold),
                ),
        );

        assert!(
            !Theme::new(&spec)
                .hint()
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }
}

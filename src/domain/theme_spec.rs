use super::pull_request::PullRequestState;
use super::sessions::session_state::SessionState;
use super::style_spec::{StyleSpec, TextModifier};
use super::theme_color::{AnsiColor, ThemeColor};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeSpec {
    overrides: ThemeOverrides,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeRole {
    Base,
    Selected,
    SelectedOutOfFocus,
    SelectedText,
    Accent,
    Dim,
    Emphasis,
    Matched,
    Error,
    Warning,
    Insertions,
    Deletions,
    Approved,
    Border,
    Title,
    DialogBorder,
    Separator,
    TableHeader,
    RepoNumber,
    RepoArrow,
    RepoName,
    BranchPrefix,
    Agent,
    Untracked,
    Hint,
    HintBarKey,
    HintBarText,
    Notice,
    Count,
    StatusInfo,
    StatusError,
    StatusNote,
    PromptCursor,
    PromptSelection,
    SettingKey,
    MainCheckout,
    MenuKey,
    MenuDisabled,
    UsageAgent,
    UsageBar,
    UsageReset,
    HelpHeading,
    PreviewTitle,
    PromptRule,
    PreviewPath,
    State(SessionState),
    PullRequest(PullRequestState),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeOverrides {
    styles: Vec<(ThemeRole, StyleSpec)>,
}

#[derive(Debug, Clone, Copy)]
enum DefaultStyle {
    Terminal,
    Own(Option<AnsiColor>, Option<TextModifier>),
    Derived(ThemeRole, Option<TextModifier>),
}

impl ThemeSpec {
    const DEFAULTS: [(ThemeRole, DefaultStyle); ThemeRole::ALL.len()] = [
        (ThemeRole::Base, DefaultStyle::Terminal),
        (
            ThemeRole::Selected,
            DefaultStyle::modified(TextModifier::Reversed),
        ),
        (
            ThemeRole::SelectedOutOfFocus,
            DefaultStyle::modified(TextModifier::Underlined),
        ),
        (
            ThemeRole::SelectedText,
            DefaultStyle::modified(TextModifier::Underlined),
        ),
        (ThemeRole::Accent, DefaultStyle::colored(AnsiColor::Cyan)),
        (ThemeRole::Dim, DefaultStyle::modified(TextModifier::Dim)),
        (
            ThemeRole::Emphasis,
            DefaultStyle::modified(TextModifier::Bold),
        ),
        (
            ThemeRole::Matched,
            DefaultStyle::Derived(ThemeRole::Emphasis, Some(TextModifier::Underlined)),
        ),
        (ThemeRole::Error, DefaultStyle::colored(AnsiColor::Red)),
        (ThemeRole::Warning, DefaultStyle::colored(AnsiColor::Yellow)),
        (
            ThemeRole::Insertions,
            DefaultStyle::colored(AnsiColor::Green),
        ),
        (ThemeRole::Deletions, DefaultStyle::colored(AnsiColor::Red)),
        (
            ThemeRole::Approved,
            DefaultStyle::Derived(ThemeRole::Insertions, Some(TextModifier::Bold)),
        ),
        (ThemeRole::Border, DefaultStyle::Own(None, None)),
        (ThemeRole::Title, DefaultStyle::Own(None, None)),
        (
            ThemeRole::DialogBorder,
            DefaultStyle::Derived(ThemeRole::Border, None),
        ),
        (
            ThemeRole::Separator,
            DefaultStyle::Derived(ThemeRole::Dim, None),
        ),
        (
            ThemeRole::TableHeader,
            DefaultStyle::Derived(ThemeRole::Emphasis, None),
        ),
        (
            ThemeRole::RepoNumber,
            DefaultStyle::Derived(ThemeRole::Dim, None),
        ),
        (ThemeRole::RepoArrow, DefaultStyle::Own(None, None)),
        (
            ThemeRole::RepoName,
            DefaultStyle::Derived(ThemeRole::Emphasis, None),
        ),
        (
            ThemeRole::BranchPrefix,
            DefaultStyle::Derived(ThemeRole::Dim, None),
        ),
        (
            ThemeRole::Agent,
            DefaultStyle::Derived(ThemeRole::Dim, None),
        ),
        (ThemeRole::Untracked, DefaultStyle::Own(None, None)),
        (ThemeRole::Hint, DefaultStyle::Derived(ThemeRole::Dim, None)),
        (ThemeRole::HintBarKey, DefaultStyle::Own(None, None)),
        (ThemeRole::HintBarText, DefaultStyle::Own(None, None)),
        (
            ThemeRole::Notice,
            DefaultStyle::Derived(ThemeRole::Dim, None),
        ),
        (
            ThemeRole::Count,
            DefaultStyle::Derived(ThemeRole::Dim, None),
        ),
        (ThemeRole::StatusInfo, DefaultStyle::Own(None, None)),
        (
            ThemeRole::StatusError,
            DefaultStyle::Derived(ThemeRole::Error, None),
        ),
        (
            ThemeRole::StatusNote,
            DefaultStyle::Derived(ThemeRole::Warning, None),
        ),
        (
            ThemeRole::PromptCursor,
            DefaultStyle::Derived(ThemeRole::Selected, None),
        ),
        (
            ThemeRole::PromptSelection,
            DefaultStyle::Derived(ThemeRole::SelectedText, None),
        ),
        (
            ThemeRole::SettingKey,
            DefaultStyle::Derived(ThemeRole::Dim, None),
        ),
        (
            ThemeRole::MainCheckout,
            DefaultStyle::Derived(ThemeRole::Warning, None),
        ),
        (
            ThemeRole::MenuKey,
            DefaultStyle::Derived(ThemeRole::Accent, None),
        ),
        (
            ThemeRole::MenuDisabled,
            DefaultStyle::Derived(ThemeRole::Dim, None),
        ),
        (
            ThemeRole::UsageAgent,
            DefaultStyle::Derived(ThemeRole::Emphasis, None),
        ),
        (ThemeRole::UsageBar, DefaultStyle::Own(None, None)),
        (
            ThemeRole::UsageReset,
            DefaultStyle::Derived(ThemeRole::Dim, None),
        ),
        (
            ThemeRole::HelpHeading,
            DefaultStyle::Derived(ThemeRole::Emphasis, None),
        ),
        (
            ThemeRole::PreviewTitle,
            DefaultStyle::Derived(ThemeRole::Emphasis, None),
        ),
        (ThemeRole::PromptRule, DefaultStyle::Own(None, None)),
        (
            ThemeRole::PreviewPath,
            DefaultStyle::Derived(ThemeRole::Dim, None),
        ),
        (
            ThemeRole::State(SessionState::Working),
            DefaultStyle::Own(None, None),
        ),
        (
            ThemeRole::State(SessionState::Waiting),
            DefaultStyle::colored(AnsiColor::Green),
        ),
        (
            ThemeRole::State(SessionState::Exited),
            DefaultStyle::colored(AnsiColor::Red),
        ),
        (
            ThemeRole::State(SessionState::Stopped),
            DefaultStyle::Derived(ThemeRole::Dim, None),
        ),
        (
            ThemeRole::State(SessionState::Unmanaged),
            DefaultStyle::Derived(ThemeRole::Warning, None),
        ),
        (
            ThemeRole::PullRequest(PullRequestState::Open),
            DefaultStyle::colored(AnsiColor::Green),
        ),
        (
            ThemeRole::PullRequest(PullRequestState::Draft),
            DefaultStyle::Derived(ThemeRole::Warning, None),
        ),
        (
            ThemeRole::PullRequest(PullRequestState::Merged),
            DefaultStyle::colored(AnsiColor::Magenta),
        ),
        (
            ThemeRole::PullRequest(PullRequestState::Closed),
            DefaultStyle::colored(AnsiColor::Red),
        ),
    ];

    pub fn new(overrides: ThemeOverrides) -> Self {
        Self { overrides }
    }

    pub fn style(&self, role: ThemeRole) -> StyleSpec {
        self.overrides
            .style(role)
            .cloned()
            .unwrap_or_else(|| self.default_style(role))
    }

    fn default_style(&self, role: ThemeRole) -> StyleSpec {
        let default = Self::DEFAULTS
            .iter()
            .find(|(defaulted, _)| *defaulted == role)
            .map_or(DefaultStyle::Own(None, None), |(_, default)| *default);
        default.resolve(self)
    }
}

impl ThemeRole {
    pub const ALL: [ThemeRole; 54] = [
        ThemeRole::Base,
        ThemeRole::Selected,
        ThemeRole::SelectedOutOfFocus,
        ThemeRole::SelectedText,
        ThemeRole::Accent,
        ThemeRole::Dim,
        ThemeRole::Emphasis,
        ThemeRole::Matched,
        ThemeRole::Error,
        ThemeRole::Warning,
        ThemeRole::Insertions,
        ThemeRole::Deletions,
        ThemeRole::Approved,
        ThemeRole::Border,
        ThemeRole::Title,
        ThemeRole::DialogBorder,
        ThemeRole::Separator,
        ThemeRole::TableHeader,
        ThemeRole::RepoNumber,
        ThemeRole::RepoArrow,
        ThemeRole::RepoName,
        ThemeRole::BranchPrefix,
        ThemeRole::Agent,
        ThemeRole::Untracked,
        ThemeRole::Hint,
        ThemeRole::HintBarKey,
        ThemeRole::HintBarText,
        ThemeRole::Notice,
        ThemeRole::Count,
        ThemeRole::StatusInfo,
        ThemeRole::StatusError,
        ThemeRole::StatusNote,
        ThemeRole::PromptCursor,
        ThemeRole::PromptSelection,
        ThemeRole::SettingKey,
        ThemeRole::MainCheckout,
        ThemeRole::MenuKey,
        ThemeRole::MenuDisabled,
        ThemeRole::UsageAgent,
        ThemeRole::UsageBar,
        ThemeRole::UsageReset,
        ThemeRole::HelpHeading,
        ThemeRole::PreviewTitle,
        ThemeRole::PromptRule,
        ThemeRole::PreviewPath,
        ThemeRole::State(SessionState::Working),
        ThemeRole::State(SessionState::Waiting),
        ThemeRole::State(SessionState::Exited),
        ThemeRole::State(SessionState::Stopped),
        ThemeRole::State(SessionState::Unmanaged),
        ThemeRole::PullRequest(PullRequestState::Open),
        ThemeRole::PullRequest(PullRequestState::Draft),
        ThemeRole::PullRequest(PullRequestState::Merged),
        ThemeRole::PullRequest(PullRequestState::Closed),
    ];
}

impl ThemeOverrides {
    pub fn with(mut self, role: ThemeRole, style: StyleSpec) -> Self {
        self.styles.retain(|(overridden, _)| *overridden != role);
        self.styles.push((role, style));
        self
    }

    fn style(&self, role: ThemeRole) -> Option<&StyleSpec> {
        self.styles
            .iter()
            .find(|(overridden, _)| *overridden == role)
            .map(|(_, style)| style)
    }
}

impl DefaultStyle {
    const fn colored(color: AnsiColor) -> Self {
        Self::Own(Some(color), None)
    }

    const fn modified(modifier: TextModifier) -> Self {
        Self::Own(None, Some(modifier))
    }

    fn resolve(self, theme: &ThemeSpec) -> StyleSpec {
        match self {
            Self::Terminal => StyleSpec::default()
                .with_foreground(ThemeColor::Default)
                .with_background(ThemeColor::Default),
            Self::Own(color, modifier) => Self::styled(StyleSpec::default(), color, modifier),
            Self::Derived(parent, modifier) => Self::styled(theme.style(parent), None, modifier),
        }
    }

    fn styled(
        style: StyleSpec,
        color: Option<AnsiColor>,
        modifier: Option<TextModifier>,
    ) -> StyleSpec {
        let colored = match color {
            Some(color) => style.with_foreground(ThemeColor::Ansi(color)),
            None => style,
        };
        match modifier {
            Some(modifier) => colored.with(modifier),
            None => colored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_role_has_a_default() {
        let defaulted: Vec<ThemeRole> = ThemeSpec::DEFAULTS.iter().map(|(role, _)| *role).collect();

        assert_eq!(defaulted, ThemeRole::ALL);
    }

    #[test]
    fn the_default_base_keeps_the_terminal_colors() {
        let base = ThemeSpec::default().style(ThemeRole::Base);

        assert_eq!(
            (base.foreground(), base.background()),
            (Some(ThemeColor::Default), Some(ThemeColor::Default))
        );
    }

    #[test]
    fn an_override_replaces_the_whole_default_style() {
        let dim = StyleSpec::default().with_foreground(ThemeColor::Indexed(240));
        let spec = ThemeSpec::new(ThemeOverrides::default().with(ThemeRole::Dim, dim.clone()));

        assert_eq!(spec.style(ThemeRole::Dim), dim);
    }

    #[test]
    fn a_derived_default_follows_its_configured_parent() {
        let warning = StyleSpec::default().with_foreground(ThemeColor::Rgb(0xd7, 0x87, 0x00));
        let spec =
            ThemeSpec::new(ThemeOverrides::default().with(ThemeRole::Warning, warning.clone()));

        assert_eq!(
            spec.style(ThemeRole::State(SessionState::Unmanaged)),
            warning
        );
    }

    #[test]
    fn a_derived_default_adds_its_own_modifier_to_the_parent() {
        let emphasis = StyleSpec::default().with_foreground(ThemeColor::Ansi(AnsiColor::White));
        let spec =
            ThemeSpec::new(ThemeOverrides::default().with(ThemeRole::Emphasis, emphasis.clone()));

        assert_eq!(
            spec.style(ThemeRole::Matched),
            emphasis.with(TextModifier::Underlined)
        );
    }

    #[test]
    fn a_later_override_of_the_same_role_wins() {
        let first = StyleSpec::default().with(TextModifier::Bold);
        let second = StyleSpec::default().with(TextModifier::Italic);
        let overrides = ThemeOverrides::default()
            .with(ThemeRole::Accent, first)
            .with(ThemeRole::Accent, second.clone());

        assert_eq!(ThemeSpec::new(overrides).style(ThemeRole::Accent), second);
    }
}

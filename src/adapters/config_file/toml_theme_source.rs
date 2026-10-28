use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::parse_failure::ParseFailure;
use crate::application::ports::{ThemeError, ThemeSource};
use crate::domain::{
    PullRequestState, SessionState, StyleSpec, TextModifier, ThemeColor, ThemeOverrides, ThemeRole,
};

pub struct TomlThemeSource {
    path: PathBuf,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct RawStyle {
    fg: Option<RawColor>,
    bg: Option<RawColor>,
    bold: Option<bool>,
    dim: Option<bool>,
    italic: Option<bool>,
    underlined: Option<bool>,
    reversed: Option<bool>,
    crossed_out: Option<bool>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawColor {
    Text(String),
    Number(i64),
    Other(toml::Value),
}

impl TomlThemeSource {
    const FILE_NAME: &'static str = "theme.toml";
    const ROLE_NAMES: [(&'static str, ThemeRole); ThemeRole::ALL.len()] = [
        ("base", ThemeRole::Base),
        ("selected", ThemeRole::Selected),
        ("selected_out_of_focus", ThemeRole::SelectedOutOfFocus),
        ("selected_text", ThemeRole::SelectedText),
        ("accent", ThemeRole::Accent),
        ("dim", ThemeRole::Dim),
        ("emphasis", ThemeRole::Emphasis),
        ("matched", ThemeRole::Matched),
        ("error", ThemeRole::Error),
        ("warning", ThemeRole::Warning),
        ("diff.insertions", ThemeRole::Insertions),
        ("diff.deletions", ThemeRole::Deletions),
        ("pull_request.approved", ThemeRole::Approved),
        ("border", ThemeRole::Border),
        ("title", ThemeRole::Title),
        ("dialog.border", ThemeRole::DialogBorder),
        ("separator", ThemeRole::Separator),
        ("table.header", ThemeRole::TableHeader),
        ("repo.name", ThemeRole::RepoName),
        ("session.branch_prefix", ThemeRole::BranchPrefix),
        ("session.agent", ThemeRole::Agent),
        ("hint", ThemeRole::Hint),
        ("hint_bar.key", ThemeRole::HintBarKey),
        ("hint_bar.text", ThemeRole::HintBarText),
        ("status.error", ThemeRole::StatusError),
        ("state.working", ThemeRole::State(SessionState::Working)),
        ("state.waiting", ThemeRole::State(SessionState::Waiting)),
        ("state.exited", ThemeRole::State(SessionState::Exited)),
        ("state.stopped", ThemeRole::State(SessionState::Stopped)),
        ("state.unmanaged", ThemeRole::State(SessionState::Unmanaged)),
        (
            "pull_request.open",
            ThemeRole::PullRequest(PullRequestState::Open),
        ),
        (
            "pull_request.draft",
            ThemeRole::PullRequest(PullRequestState::Draft),
        ),
        (
            "pull_request.merged",
            ThemeRole::PullRequest(PullRequestState::Merged),
        ),
        (
            "pull_request.closed",
            ThemeRole::PullRequest(PullRequestState::Closed),
        ),
    ];

    pub fn beside(config_path: &Path) -> Self {
        Self {
            path: config_path.with_file_name(Self::FILE_NAME),
        }
    }

    fn parse(&self, text: &str) -> Result<ThemeOverrides, ThemeError> {
        let table: toml::Table = toml::from_str(text).map_err(|error| ThemeError::Parse {
            path: self.path.clone(),
            detail: ParseFailure::new(text, &error).describe(),
        })?;
        let (overrides, failures) = Self::overrides(table);
        if failures.is_empty() {
            Ok(overrides)
        } else {
            Err(ThemeError::Invalid {
                path: self.path.clone(),
                failures,
            })
        }
    }

    fn overrides(table: toml::Table) -> (ThemeOverrides, Vec<String>) {
        let mut overrides = ThemeOverrides::default();
        let mut failures = Vec::new();
        for (name, value) in Self::entries(table) {
            match Self::role_style(&name, value) {
                Ok((role, style)) => overrides = overrides.with(role, style),
                Err(failure) => failures.push(failure),
            }
        }
        (overrides, failures)
    }

    fn entries(table: toml::Table) -> Vec<(String, toml::Value)> {
        table
            .into_iter()
            .flat_map(|(key, value)| match value {
                toml::Value::Table(group) if Self::is_group(&key) => group
                    .into_iter()
                    .map(|(role, value)| (format!("{key}.{role}"), value))
                    .collect(),
                value => vec![(key, value)],
            })
            .collect()
    }

    fn is_group(key: &str) -> bool {
        let prefix = format!("{key}.");
        Self::ROLE_NAMES
            .iter()
            .any(|(name, _)| name.starts_with(&prefix))
    }

    fn role_style(name: &str, value: toml::Value) -> Result<(ThemeRole, StyleSpec), String> {
        let role = Self::role_named(name).ok_or_else(|| Self::unknown_role(name))?;
        let raw: RawStyle = value
            .try_into()
            .map_err(|error| format!("`{name}`: {error}"))?;
        let style = raw
            .into_style()
            .map_err(|failure| format!("`{name}`: {failure}"))?;
        Ok((role, style))
    }

    fn unknown_role(name: &str) -> String {
        let unknown = format!("unknown theme role `{name}`");
        let group = name.split_once('.').map_or(name, |(group, _)| group);
        if Self::is_group(group) {
            format!("{unknown}: `[{group}]` takes {}", Self::roles_in(group))
        } else {
            unknown
        }
    }

    fn roles_in(group: &str) -> String {
        let prefix = format!("{group}.");
        let roles: Vec<&str> = Self::ROLE_NAMES
            .iter()
            .filter_map(|(name, _)| name.strip_prefix(&prefix))
            .collect();
        roles.join(", ")
    }

    fn role_named(name: &str) -> Option<ThemeRole> {
        Self::ROLE_NAMES
            .iter()
            .find(|(listed, _)| *listed == name)
            .map(|(_, role)| *role)
    }

    fn io_error(&self, error: &io::Error) -> ThemeError {
        ThemeError::Io {
            path: self.path.clone(),
            detail: error.to_string(),
        }
    }
}

impl ThemeSource for TomlThemeSource {
    fn load(&self) -> Result<ThemeOverrides, ThemeError> {
        match fs::read_to_string(&self.path) {
            Ok(text) => self.parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(ThemeOverrides::default()),
            Err(error) => Err(self.io_error(&error)),
        }
    }
}

impl RawStyle {
    fn into_style(self) -> Result<StyleSpec, String> {
        let foreground = self.fg.as_ref().map(RawColor::parse).transpose()?;
        let background = self.bg.as_ref().map(RawColor::parse).transpose()?;
        let style = foreground
            .into_iter()
            .fold(self.modified_style(), StyleSpec::with_foreground);
        Ok(background
            .into_iter()
            .fold(style, StyleSpec::with_background))
    }

    fn modified_style(&self) -> StyleSpec {
        [
            (self.bold, TextModifier::Bold),
            (self.dim, TextModifier::Dim),
            (self.italic, TextModifier::Italic),
            (self.underlined, TextModifier::Underlined),
            (self.reversed, TextModifier::Reversed),
            (self.crossed_out, TextModifier::CrossedOut),
        ]
        .into_iter()
        .fold(
            StyleSpec::default(),
            |style, (setting, modifier)| match setting {
                Some(true) => style.with(modifier),
                Some(false) => style.without(modifier),
                None => style,
            },
        )
    }
}

impl RawColor {
    fn parse(&self) -> Result<ThemeColor, String> {
        match self {
            Self::Text(text) => ThemeColor::parse(text),
            Self::Number(number) => ThemeColor::parse(&number.to_string()),
            Self::Other(value) => ThemeColor::parse(&value.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AnsiColor, ThemeSpec};

    const FIXTURE: &str = include_str!("../../../tests/fixtures/theme.toml");

    struct ThemeFileFixture {
        directory: tempfile::TempDir,
    }

    impl ThemeFileFixture {
        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn containing(text: &str) -> Self {
            let fixture = Self::new();
            fs::write(fixture.directory.path().join("theme.toml"), text).unwrap();
            fixture
        }

        fn load(&self) -> Result<ThemeOverrides, ThemeError> {
            TomlThemeSource::beside(&self.directory.path().join("config.toml")).load()
        }

        fn spec(&self) -> ThemeSpec {
            ThemeSpec::new(self.load().expect("the theme loads"))
        }

        fn failures(&self) -> Vec<String> {
            match self.load() {
                Err(ThemeError::Invalid { failures, .. }) => failures,
                other => panic!("expected an invalid theme, got {other:?}"),
            }
        }
    }

    #[test]
    fn every_role_has_one_name_so_each_role_stays_configurable() {
        let named: Vec<ThemeRole> = TomlThemeSource::ROLE_NAMES
            .iter()
            .map(|(_, role)| *role)
            .collect();

        assert_eq!(named, ThemeRole::ALL);
    }

    #[test]
    fn a_missing_file_overrides_nothing() {
        assert_eq!(
            ThemeFileFixture::new().load(),
            Ok(ThemeOverrides::default())
        );
    }

    #[test]
    fn the_fixture_sets_a_top_level_role() {
        let selected = ThemeFileFixture::containing(FIXTURE)
            .spec()
            .style(ThemeRole::Selected);

        assert_eq!(
            selected,
            StyleSpec::default()
                .with(TextModifier::Bold)
                .with_background(ThemeColor::Rgb(0x33, 0x33, 0x33))
        );
    }

    #[test]
    fn the_fixture_sets_a_grouped_role() {
        let merged = ThemeFileFixture::containing(FIXTURE)
            .spec()
            .style(ThemeRole::PullRequest(PullRequestState::Merged));

        assert_eq!(
            merged,
            StyleSpec::default()
                .with(TextModifier::Bold)
                .with_foreground(ThemeColor::Ansi(AnsiColor::Magenta))
        );
    }

    #[test]
    fn a_top_level_role_and_a_group_with_a_similar_name_load_side_by_side() {
        let spec = ThemeFileFixture::containing(
            "hint = { italic = true }\n[hint_bar]\nkey = { bold = true }\n",
        )
        .spec();

        assert_eq!(
            (
                spec.style(ThemeRole::Hint),
                spec.style(ThemeRole::HintBarKey)
            ),
            (
                StyleSpec::default().with(TextModifier::Italic),
                StyleSpec::default().with(TextModifier::Bold)
            )
        );
    }

    #[test]
    fn an_unknown_role_inside_a_group_names_the_dotted_role() {
        let failures = ThemeFileFixture::containing("[repo]\nnmae = { bold = true }\n").failures();

        assert_eq!(
            failures,
            ["unknown theme role `repo.nmae`: `[repo]` takes name"]
        );
    }

    #[test]
    fn a_style_written_on_a_group_points_at_the_groups_roles() {
        let failures = ThemeFileFixture::containing("state = { fg = \"red\" }\n").failures();

        assert_eq!(
            failures,
            [
                "unknown theme role `state.fg`: `[state]` takes working, waiting, exited, stopped, unmanaged"
            ]
        );
    }

    #[test]
    fn a_plain_value_on_a_group_points_at_the_groups_roles() {
        let failures = ThemeFileFixture::containing("pull_request = \"red\"\n").failures();

        assert_eq!(
            failures,
            [
                "unknown theme role `pull_request`: `[pull_request]` takes approved, open, draft, merged, closed"
            ]
        );
    }

    #[test]
    fn an_unknown_role_is_refused_so_a_typo_does_not_pass_silently() {
        let failures = ThemeFileFixture::containing("acent = { fg = \"red\" }\n").failures();

        assert_eq!(failures, ["unknown theme role `acent`"]);
    }

    #[test]
    fn a_false_modifier_removes_it() {
        let spec = ThemeFileFixture::containing("hint = { bold = false }\n").spec();

        assert_eq!(
            spec.style(ThemeRole::Hint),
            StyleSpec::default().without(TextModifier::Bold)
        );
    }

    #[test]
    fn a_bare_number_picks_a_256_color_index() {
        let spec = ThemeFileFixture::containing("dim = { fg = 240 }\n").spec();

        assert_eq!(
            spec.style(ThemeRole::Dim),
            StyleSpec::default().with_foreground(ThemeColor::Indexed(240))
        );
    }

    #[test]
    fn a_bare_number_past_255_is_refused() {
        let failures = ThemeFileFixture::containing("dim = { fg = 300 }\n").failures();

        assert!(failures[0].contains("`300` is not a color"), "{failures:?}");
    }

    #[test]
    fn a_color_of_another_type_lists_the_accepted_forms() {
        let failures = ThemeFileFixture::containing("dim = { fg = 2.5 }\n").failures();

        assert!(
            failures[0].contains("`2.5` is not a color: use"),
            "{failures:?}"
        );
    }

    #[test]
    fn an_unknown_style_key_is_refused() {
        let failures = ThemeFileFixture::containing("accent = { bolt = true }\n").failures();

        assert!(failures[0].contains("bolt"), "{failures:?}");
    }

    #[test]
    fn every_bad_role_is_reported() {
        let failures = ThemeFileFixture::containing(
            "accent = { fg = \"teal\" }\n[state]\nexited = { bg = \"#12\" }\n",
        )
        .failures();

        assert_eq!(failures.len(), 2, "{failures:?}");
    }

    #[test]
    fn a_syntax_error_names_the_offending_line() {
        let error = ThemeFileFixture::containing("accent = { fg = \n").load();

        assert!(
            matches!(&error, Err(ThemeError::Parse { detail, .. }) if detail.contains("accent")),
            "{error:?}"
        );
    }
}

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::built_in_skill::BuiltInSkill;

pub struct BuiltInSkillOptions {
    plugin: PathBuf,
}

impl BuiltInSkillOptions {
    const CLAUDE: &'static str = "claude";
    const OPENCODE: &'static str = "opencode";
    const PLUGIN_DIRECTORY_FLAG: &'static str = "--plugin-dir";
    const OPENCODE_CONFIG: &'static str = "OPENCODE_CONFIG_CONTENT";
    const SKILLS_KEY: &'static str = "skills";
    const PATHS_KEY: &'static str = "paths";

    pub fn new(plugin: &Path) -> Self {
        Self {
            plugin: plugin.to_path_buf(),
        }
    }

    pub fn argv(&self, argv: Vec<OsString>) -> Vec<OsString> {
        if !Self::runs(&argv, Self::CLAUDE) {
            return argv;
        }
        let mut words = argv.into_iter();
        let program = words.next();
        let flag = [
            OsString::from(Self::PLUGIN_DIRECTORY_FLAG),
            self.plugin.clone().into_os_string(),
        ];
        program.into_iter().chain(flag).chain(words).collect()
    }

    pub fn environment(
        &self,
        argv: &[OsString],
        environment: Vec<(OsString, OsString)>,
    ) -> Vec<(OsString, OsString)> {
        if !Self::runs(argv, Self::OPENCODE) {
            return environment;
        }
        let existing = Self::value_of(&environment, Self::OPENCODE_CONFIG);
        match self.opencode_config(existing) {
            Some(config) => Self::with_variable(environment, Self::OPENCODE_CONFIG, config),
            None => environment,
        }
    }

    fn runs(argv: &[OsString], program: &str) -> bool {
        argv.first()
            .and_then(|command| Path::new(command).file_name())
            .is_some_and(|name| name == program)
    }

    fn value_of<'e>(environment: &'e [(OsString, OsString)], name: &str) -> Option<&'e OsStr> {
        environment
            .iter()
            .rev()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value.as_os_str())
    }

    fn opencode_config(&self, existing: Option<&OsStr>) -> Option<String> {
        let mut config = Self::parsed(existing)?;
        let skills_path = self.plugin.join(BuiltInSkill::SKILLS_DIRECTORY);
        let skills_path = Value::from(skills_path.to_str()?);
        Self::skill_paths(config.as_object_mut()?)?.push(skills_path);
        Some(config.to_string())
    }

    fn parsed(existing: Option<&OsStr>) -> Option<Value> {
        match existing {
            Some(text) => serde_json::from_str(text.to_str()?).ok(),
            None => Some(Value::Object(Map::new())),
        }
    }

    fn skill_paths(config: &mut Map<String, Value>) -> Option<&mut Vec<Value>> {
        let skills = config
            .entry(Self::SKILLS_KEY)
            .or_insert_with(|| Value::Object(Map::new()));
        match skills {
            Value::Array(paths) => Some(paths),
            Value::Object(skills) => Self::listed_paths(skills),
            _ => None,
        }
    }

    fn listed_paths(skills: &mut Map<String, Value>) -> Option<&mut Vec<Value>> {
        skills
            .entry(Self::PATHS_KEY)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
    }

    fn with_variable(
        environment: Vec<(OsString, OsString)>,
        name: &str,
        value: String,
    ) -> Vec<(OsString, OsString)> {
        environment
            .into_iter()
            .filter(|(candidate, _)| candidate != name)
            .chain([(OsString::from(name), OsString::from(value))])
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OptionsFixture;

    impl OptionsFixture {
        const PLUGIN: &'static str = "/data/ringleader/plugin/0123456789abcdef";
        const SKILLS: &'static str = "/data/ringleader/plugin/0123456789abcdef/skills";

        fn options() -> BuiltInSkillOptions {
            BuiltInSkillOptions::new(Path::new(Self::PLUGIN))
        }

        fn words(words: &[&str]) -> Vec<OsString> {
            words.iter().map(OsString::from).collect()
        }

        fn opencode_with(value: &str) -> Vec<(OsString, OsString)> {
            vec![(
                OsString::from(BuiltInSkillOptions::OPENCODE_CONFIG),
                OsString::from(value),
            )]
        }

        fn opencode_config(environment: Vec<(OsString, OsString)>) -> Option<Value> {
            let given = Self::options().environment(&Self::words(&["opencode"]), environment);
            let value =
                BuiltInSkillOptions::value_of(&given, BuiltInSkillOptions::OPENCODE_CONFIG)?;
            serde_json::from_str(value.to_str()?).ok()
        }
    }

    #[test]
    fn claude_gets_the_plugin_right_after_its_program() {
        let argv = OptionsFixture::words(&["claude", "--name", "fix-auth", "review it"]);

        let given = OptionsFixture::options().argv(argv);

        assert_eq!(
            given,
            OptionsFixture::words(&[
                "claude",
                "--plugin-dir",
                OptionsFixture::PLUGIN,
                "--name",
                "fix-auth",
                "review it",
            ])
        );
    }

    #[test]
    fn claude_named_by_its_full_path_gets_the_plugin() {
        let argv = OptionsFixture::words(&["/nix/store/example-claude/bin/claude"]);

        let given = OptionsFixture::options().argv(argv);

        assert_eq!(given[1], "--plugin-dir");
    }

    #[test]
    fn another_program_keeps_its_argv() {
        let argv = OptionsFixture::words(&["opencode", "--prompt=review it"]);

        let given = OptionsFixture::options().argv(argv.clone());

        assert_eq!(given, argv);
    }

    #[test]
    fn opencode_gets_the_skill_folder_in_its_config_content() {
        let config = OptionsFixture::opencode_config(Vec::new()).unwrap();

        assert_eq!(
            config,
            serde_json::json!({ "skills": { "paths": [OptionsFixture::SKILLS] } })
        );
    }

    #[test]
    fn opencode_config_content_the_user_set_keeps_its_other_keys() {
        let environment = OptionsFixture::opencode_with(r#"{"model":"example/model"}"#);

        let config = OptionsFixture::opencode_config(environment).unwrap();

        assert_eq!(config["model"], "example/model");
        assert_eq!(config["skills"]["paths"][0], OptionsFixture::SKILLS);
    }

    #[test]
    fn opencode_skill_paths_the_user_set_gain_the_skill_folder() {
        let environment = OptionsFixture::opencode_with(r#"{"skills":{"paths":["/team"]}}"#);

        let config = OptionsFixture::opencode_config(environment).unwrap();

        assert_eq!(
            config["skills"]["paths"],
            serde_json::json!(["/team", OptionsFixture::SKILLS])
        );
    }

    #[test]
    fn an_opencode_skill_list_the_user_set_gains_the_skill_folder() {
        let environment = OptionsFixture::opencode_with(r#"{"skills":["/team"]}"#);

        let config = OptionsFixture::opencode_config(environment).unwrap();

        assert_eq!(
            config["skills"],
            serde_json::json!(["/team", OptionsFixture::SKILLS])
        );
    }

    #[test]
    fn opencode_config_content_that_is_not_json_is_left_alone() {
        let environment = OptionsFixture::opencode_with("not json");

        let given = OptionsFixture::options()
            .environment(&OptionsFixture::words(&["opencode"]), environment.clone());

        assert_eq!(given, environment);
    }

    #[test]
    fn opencode_skills_of_an_unknown_shape_are_left_alone() {
        let environment = OptionsFixture::opencode_with(r#"{"skills":true}"#);

        let given = OptionsFixture::options()
            .environment(&OptionsFixture::words(&["opencode"]), environment.clone());

        assert_eq!(given, environment);
    }

    #[test]
    fn another_program_keeps_its_environment() {
        let environment = OptionsFixture::opencode_with(r#"{"model":"example/model"}"#);

        let given = OptionsFixture::options()
            .environment(&OptionsFixture::words(&["claude"]), environment.clone());

        assert_eq!(given, environment);
    }
}

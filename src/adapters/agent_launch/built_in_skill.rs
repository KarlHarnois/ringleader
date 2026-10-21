use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

pub struct BuiltInSkill {
    root: PathBuf,
}

impl BuiltInSkill {
    pub const SKILLS_DIRECTORY: &'static str = "skills";
    const SKILL: &'static str =
        include_str!("../../../skills/managing-ringleader-sessions/SKILL.md");
    const SKILL_NAME: &'static str = "managing-ringleader-sessions";
    const SKILL_FILE: &'static str = "SKILL.md";
    const PLUGIN_NAME: &'static str = "ringleader";
    const PLUGIN_DESCRIPTION: &'static str =
        "Start, prompt, and manage Ringleader sessions from inside one";
    const MANIFEST_DIRECTORY: &'static str = ".claude-plugin";
    const MANIFEST_FILE: &'static str = "plugin.json";
    const STAGING_PREFIX: &'static str = ".staging-";

    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    pub fn publish(&self) -> io::Result<PathBuf> {
        let published = self.root.join(Self::digest());
        if !published.is_dir() {
            self.publish_into(&published)?;
        }
        Ok(published)
    }

    fn publish_into(&self, published: &Path) -> io::Result<()> {
        let staging = self
            .root
            .join(format!("{}{}", Self::STAGING_PREFIX, std::process::id()));
        let moved = Self::stage(&staging).and_then(|()| fs::rename(&staging, published));
        if moved.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        moved.or_else(|error| Self::published_by_another_process(published, error))
    }

    fn published_by_another_process(published: &Path, error: io::Error) -> io::Result<()> {
        match published.is_dir() {
            true => Ok(()),
            false => Err(error),
        }
    }

    fn stage(staging: &Path) -> io::Result<()> {
        let _ = fs::remove_dir_all(staging);
        let manifest_directory = staging.join(Self::MANIFEST_DIRECTORY);
        let skill_directory = staging.join(Self::SKILLS_DIRECTORY).join(Self::SKILL_NAME);
        fs::create_dir_all(&manifest_directory)?;
        fs::create_dir_all(&skill_directory)?;
        fs::write(
            manifest_directory.join(Self::MANIFEST_FILE),
            Self::manifest(),
        )?;
        fs::write(skill_directory.join(Self::SKILL_FILE), Self::SKILL)
    }

    fn manifest() -> String {
        serde_json::json!({
            "name": Self::PLUGIN_NAME,
            "version": env!("CARGO_PKG_VERSION"),
            "description": Self::PLUGIN_DESCRIPTION,
        })
        .to_string()
    }

    fn digest() -> String {
        let mut hasher = DefaultHasher::new();
        Self::SKILL.hash(&mut hasher);
        Self::manifest().hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SkillFixture {
        directory: tempfile::TempDir,
    }

    impl SkillFixture {
        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn skill(&self) -> BuiltInSkill {
            BuiltInSkill::new(&self.directory.path().join("plugin"))
        }

        fn manifest(published: &Path) -> serde_json::Value {
            let text = fs::read_to_string(
                published
                    .join(BuiltInSkill::MANIFEST_DIRECTORY)
                    .join(BuiltInSkill::MANIFEST_FILE),
            )
            .unwrap();
            serde_json::from_str(&text).unwrap()
        }

        fn skill_text(published: &Path) -> String {
            fs::read_to_string(
                published
                    .join(BuiltInSkill::SKILLS_DIRECTORY)
                    .join(BuiltInSkill::SKILL_NAME)
                    .join(BuiltInSkill::SKILL_FILE),
            )
            .unwrap()
        }

        fn entries(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(self.directory.path().join("plugin"))
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    #[test]
    fn a_published_skill_is_a_plugin_named_ringleader() {
        let fixture = SkillFixture::new();

        let published = fixture.skill().publish().unwrap();

        assert_eq!(SkillFixture::manifest(&published)["name"], "ringleader");
    }

    #[test]
    fn a_published_skill_holds_the_embedded_skill_text() {
        let fixture = SkillFixture::new();

        let published = fixture.skill().publish().unwrap();

        assert_eq!(SkillFixture::skill_text(&published), BuiltInSkill::SKILL);
    }

    #[test]
    fn a_second_publish_reuses_the_published_folder() {
        let fixture = SkillFixture::new();
        let first = fixture.skill().publish().unwrap();

        let second = fixture.skill().publish().unwrap();

        assert_eq!(first, second);
        assert_eq!(fixture.entries().len(), 1);
    }

    #[test]
    fn a_publish_leaves_no_staging_folder_behind() {
        let fixture = SkillFixture::new();

        fixture.skill().publish().unwrap();

        assert!(
            fixture
                .entries()
                .iter()
                .all(|name| !name.starts_with(BuiltInSkill::STAGING_PREFIX))
        );
    }

    #[test]
    fn a_publish_under_a_file_fails() {
        let fixture = SkillFixture::new();
        let blocker = fixture.directory.path().join("blocker");
        fs::write(&blocker, "").unwrap();

        let published = BuiltInSkill::new(&blocker.join("plugin")).publish();

        assert!(published.is_err());
    }
}

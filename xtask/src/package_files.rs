use crate::release_version::ReleaseVersion;
use crate::toml_text::TomlText;

pub struct PackageFiles {
    manifest: TomlText,
    lock: TomlText,
}

impl PackageFiles {
    const PACKAGE_TABLE: &'static str = "[package]";
    const NAME_KEY: &'static str = "name";
    const VERSION_KEY: &'static str = "version";
    const MANIFEST_NAME: &'static str = "the manifest";
    const LOCK_NAME: &'static str = "the lock file";

    pub fn new(manifest: TomlText, lock: TomlText) -> Self {
        Self { manifest, lock }
    }

    pub fn manifest(&self) -> &str {
        self.manifest.as_str()
    }

    pub fn lock(&self) -> &str {
        self.lock.as_str()
    }

    pub fn versioned(&self, version: &ReleaseVersion) -> Result<Self, String> {
        let version = version.to_string();
        let lock_entry = self.lock_entry()?;
        let manifest = self
            .manifest
            .with_string_value(Self::PACKAGE_TABLE, Self::VERSION_KEY, &version)
            .ok_or_else(|| {
                Self::key_missing(Self::MANIFEST_NAME, Self::PACKAGE_TABLE, Self::VERSION_KEY)
            })?;
        let lock = self
            .lock
            .with_string_value(&lock_entry, Self::VERSION_KEY, &version)
            .ok_or_else(|| Self::key_missing(Self::LOCK_NAME, &lock_entry, Self::VERSION_KEY))?;
        Ok(Self::new(manifest, lock))
    }

    fn lock_entry(&self) -> Result<String, String> {
        let name = self
            .manifest
            .string_value(Self::PACKAGE_TABLE, Self::NAME_KEY)
            .ok_or_else(|| {
                Self::key_missing(Self::MANIFEST_NAME, Self::PACKAGE_TABLE, Self::NAME_KEY)
            })?;
        Ok(format!("{} = \"{name}\"", Self::NAME_KEY))
    }

    fn key_missing(file: &str, anchor: &str, key: &str) -> String {
        format!("Setting the crate version failed: {file} has no `{key}` under `{anchor}`")
    }
}

#[cfg(test)]
mod tests {
    use super::PackageFiles;
    use crate::release_version::ReleaseVersion;
    use crate::toml_text::TomlText;

    struct PackageFilesFixture;

    impl PackageFilesFixture {
        const MANIFEST: &'static str = include_str!("../fixtures/release/manifest.toml");
        const LOCK: &'static str = include_str!("../fixtures/release/lock.toml");
        const VERSIONED_MANIFEST: &'static str =
            include_str!("../fixtures/release/versioned-manifest.toml");
        const VERSIONED_LOCK: &'static str =
            include_str!("../fixtures/release/versioned-lock.toml");

        fn files(manifest: &str) -> PackageFiles {
            PackageFiles::new(
                TomlText::new(manifest.to_string()),
                TomlText::new(Self::LOCK.to_string()),
            )
        }

        fn versioned(version: &str) -> PackageFiles {
            Self::files(Self::MANIFEST)
                .versioned(&ReleaseVersion::parse(version).unwrap())
                .unwrap()
        }
    }

    #[test]
    fn the_manifest_changes_only_the_package_version() {
        assert_eq!(
            PackageFilesFixture::versioned("0.1.1").manifest(),
            PackageFilesFixture::VERSIONED_MANIFEST
        );
    }

    #[test]
    fn the_lock_file_changes_only_the_package_entry() {
        assert_eq!(
            PackageFilesFixture::versioned("0.1.1").lock(),
            PackageFilesFixture::VERSIONED_LOCK
        );
    }

    #[test]
    fn a_manifest_without_a_package_name_is_refused() {
        let files = PackageFilesFixture::files("[package]\nversion = \"0.1.0\"\n");

        assert!(
            files
                .versioned(&ReleaseVersion::parse("0.1.1").unwrap())
                .is_err()
        );
    }
}

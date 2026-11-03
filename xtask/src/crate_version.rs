use std::fs;
use std::path::{Path, PathBuf};

use crate::package_files::PackageFiles;
use crate::release_version::ReleaseVersion;
use crate::toml_text::TomlText;

pub struct CrateVersion {
    manifest: PathBuf,
    lock: PathBuf,
}

impl CrateVersion {
    const MANIFEST: &'static str = "Cargo.toml";
    const LOCK: &'static str = "Cargo.lock";

    pub fn new(root: &Path) -> Self {
        Self {
            manifest: root.join(Self::MANIFEST),
            lock: root.join(Self::LOCK),
        }
    }

    pub fn set(&self, version: &ReleaseVersion) -> Result<(), String> {
        let files = PackageFiles::new(Self::read(&self.manifest)?, Self::read(&self.lock)?);
        let versioned = files.versioned(version)?;
        Self::write(&self.manifest, versioned.manifest())?;
        Self::write(&self.lock, versioned.lock())
    }

    fn read(path: &Path) -> Result<TomlText, String> {
        fs::read_to_string(path)
            .map(TomlText::new)
            .map_err(|error| format!("Reading `{}` failed: {error}", path.display()))
    }

    fn write(path: &Path, text: &str) -> Result<(), String> {
        fs::write(path, text)
            .map_err(|error| format!("Writing `{}` failed: {error}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::CrateVersion;
    use crate::release_version::ReleaseVersion;

    struct CrateFixture {
        root: tempfile::TempDir,
    }

    impl CrateFixture {
        const MANIFEST: &'static str = include_str!("../fixtures/release/manifest.toml");
        const LOCK: &'static str = include_str!("../fixtures/release/lock.toml");

        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            fs::write(root.path().join("Cargo.toml"), Self::MANIFEST).unwrap();
            fs::write(root.path().join("Cargo.lock"), Self::LOCK).unwrap();
            Self { root }
        }

        fn set(&self, version: &str) {
            CrateVersion::new(self.root.path())
                .set(&ReleaseVersion::parse(version).unwrap())
                .unwrap();
        }

        fn manifest(&self) -> String {
            fs::read_to_string(self.root.path().join("Cargo.toml")).unwrap()
        }
    }

    #[test]
    fn setting_the_version_rewrites_the_manifest_on_disk() {
        let fixture = CrateFixture::new();

        fixture.set("0.2.0");

        assert!(fixture.manifest().contains("version = \"0.2.0\""));
    }
}

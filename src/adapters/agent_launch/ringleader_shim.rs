use std::ffi::OsString;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

pub struct RingleaderShim {
    root: PathBuf,
    executable: PathBuf,
}

impl RingleaderShim {
    const PROGRAM: &'static str = "ringleader";
    const PATH_VARIABLE: &'static str = "PATH";
    const PATH_SEPARATOR: &'static str = ":";
    const STAGING_PREFIX: &'static str = ".staging-";

    pub fn new(root: &Path, executable: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            executable: executable.to_path_buf(),
        }
    }

    pub fn publish(&self) -> io::Result<PathBuf> {
        let folder = self.root.join(self.digest());
        if fs::read_link(folder.join(Self::PROGRAM)).ok() != Some(self.executable.clone()) {
            self.link_into(&folder)?;
        }
        Ok(folder)
    }

    pub fn on_path(
        folder: &Path,
        environment: Vec<(OsString, OsString)>,
    ) -> Vec<(OsString, OsString)> {
        let mut path = folder.as_os_str().to_os_string();
        if let Some(inherited) = Self::inherited_path(&environment) {
            path.push(Self::PATH_SEPARATOR);
            path.push(inherited);
        }
        environment
            .into_iter()
            .filter(|(name, _)| name != Self::PATH_VARIABLE)
            .chain([(OsString::from(Self::PATH_VARIABLE), path)])
            .collect()
    }

    fn inherited_path(environment: &[(OsString, OsString)]) -> Option<OsString> {
        environment
            .iter()
            .rev()
            .find(|(name, _)| name == Self::PATH_VARIABLE)
            .map(|(_, value)| value.clone())
    }

    fn link_into(&self, folder: &Path) -> io::Result<()> {
        fs::create_dir_all(folder)?;
        let staging = folder.join(format!("{}{}", Self::STAGING_PREFIX, std::process::id()));
        let _ = fs::remove_file(&staging);
        symlink(&self.executable, &staging)?;
        fs::rename(&staging, folder.join(Self::PROGRAM))
    }

    fn digest(&self) -> String {
        let mut hasher = DefaultHasher::new();
        self.executable.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ShimFixture {
        directory: tempfile::TempDir,
    }

    impl ShimFixture {
        const BUILD: &'static str = "/builds/one/ringleader";
        const OTHER_BUILD: &'static str = "/builds/two/ringleader";
        const FOLDER: &'static str = "/run/ringleader/bin/0123456789abcdef";

        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn shim(&self, executable: &str) -> RingleaderShim {
            RingleaderShim::new(&self.directory.path().join("bin"), Path::new(executable))
        }

        fn path_of(environment: &[(OsString, OsString)]) -> Vec<OsString> {
            environment
                .iter()
                .filter(|(name, _)| name == "PATH")
                .map(|(_, value)| value.clone())
                .collect()
        }

        fn pair(name: &str, value: &str) -> (OsString, OsString) {
            (OsString::from(name), OsString::from(value))
        }
    }

    #[test]
    fn a_published_shim_links_ringleader_to_the_running_build() {
        let fixture = ShimFixture::new();

        let folder = fixture.shim(ShimFixture::BUILD).publish().unwrap();

        assert_eq!(
            fs::read_link(folder.join("ringleader")).unwrap(),
            Path::new(ShimFixture::BUILD)
        );
    }

    #[test]
    fn a_published_shim_holds_nothing_but_the_link() {
        let fixture = ShimFixture::new();

        let folder = fixture.shim(ShimFixture::BUILD).publish().unwrap();

        let names: Vec<OsString> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["ringleader"]);
    }

    #[test]
    fn a_second_publish_of_one_build_reuses_its_folder() {
        let fixture = ShimFixture::new();
        let first = fixture.shim(ShimFixture::BUILD).publish().unwrap();

        let second = fixture.shim(ShimFixture::BUILD).publish().unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn two_builds_get_two_folders() {
        let fixture = ShimFixture::new();
        let first = fixture.shim(ShimFixture::BUILD).publish().unwrap();

        let second = fixture.shim(ShimFixture::OTHER_BUILD).publish().unwrap();

        assert_ne!(first, second);
        assert_eq!(
            fs::read_link(first.join("ringleader")).unwrap(),
            Path::new(ShimFixture::BUILD)
        );
    }

    #[test]
    fn a_publish_under_a_file_fails() {
        let fixture = ShimFixture::new();
        let blocker = fixture.directory.path().join("blocker");
        fs::write(&blocker, "").unwrap();

        let published =
            RingleaderShim::new(&blocker.join("bin"), Path::new(ShimFixture::BUILD)).publish();

        assert!(published.is_err());
    }

    #[test]
    fn the_shim_folder_goes_first_on_the_path_the_agent_inherits() {
        let environment = vec![
            ShimFixture::pair("PATH", "/devshell/bin:/usr/bin"),
            ShimFixture::pair("HOME", "/home/example"),
        ];

        let given = RingleaderShim::on_path(Path::new(ShimFixture::FOLDER), environment);

        assert_eq!(
            ShimFixture::path_of(&given),
            [OsString::from(
                "/run/ringleader/bin/0123456789abcdef:/devshell/bin:/usr/bin"
            )]
        );
    }

    #[test]
    fn an_agent_without_a_path_gets_the_shim_folder_alone() {
        let given = RingleaderShim::on_path(Path::new(ShimFixture::FOLDER), Vec::new());

        assert_eq!(
            ShimFixture::path_of(&given),
            [OsString::from(ShimFixture::FOLDER)]
        );
    }
}

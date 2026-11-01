use std::ffi::{OsStr, OsString};
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use crate::adapters::filesystem::atomic_file::AtomicFile;
use crate::adapters::git::shell_word::ShellWord;

pub struct RingleaderShim {
    root: PathBuf,
    executable: PathBuf,
    assignments: Vec<OsString>,
}

impl RingleaderShim {
    const PROGRAM: &'static str = "ringleader";
    const PATH_VARIABLE: &'static str = "PATH";
    const PATH_SEPARATOR: &'static str = ":";
    const INTERPRETER: &'static str = "#!/bin/sh";
    const EXECUTABLE_MODE: u32 = 0o755;

    pub fn new(root: &Path, executable: &Path, assignments: &[OsString]) -> Self {
        Self {
            root: root.to_path_buf(),
            executable: executable.to_path_buf(),
            assignments: assignments.to_vec(),
        }
    }

    pub fn publish(&self) -> io::Result<PathBuf> {
        let script = self.script()?;
        let folder = self.root.join(Self::digest(&script));
        if !Self::holds(&folder, &script) {
            Self::write_into(&folder, &script)?;
        }
        Ok(folder)
    }

    fn holds(folder: &Path, script: &str) -> bool {
        fs::read(folder.join(Self::PROGRAM)).ok().as_deref() == Some(script.as_bytes())
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

    fn script(&self) -> io::Result<String> {
        self.require_absolute_build()?;
        let mut lines = vec![Self::INTERPRETER.to_string()];
        for assignment in &self.assignments {
            lines.push(format!("export {}", Self::shell_word(assignment)?));
        }
        let build = Self::shell_word(self.executable.as_os_str())?;
        lines.push(format!("exec {build} \"$@\""));
        Ok(lines.join("\n") + "\n")
    }

    fn require_absolute_build(&self) -> io::Result<()> {
        if self.executable.is_absolute() {
            return Ok(());
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "cannot write {} into the ringleader shim, it is not an absolute path",
                self.executable.display()
            ),
        ))
    }

    fn shell_word(text: &OsStr) -> io::Result<String> {
        let text = text.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "cannot write {} into the ringleader shim, it is not UTF-8",
                    text.to_string_lossy()
                ),
            )
        })?;
        Ok(ShellWord::new(text).to_string())
    }

    fn write_into(folder: &Path, script: &str) -> io::Result<()> {
        fs::create_dir_all(folder)?;
        AtomicFile::new(&folder.join(Self::PROGRAM)).replace(
            script.as_bytes(),
            Self::EXECUTABLE_MODE,
            |_| Ok(()),
        )
    }

    fn digest(script: &str) -> String {
        let mut hasher = DefaultHasher::new();
        script.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    use super::*;
    use crate::adapters::host::process_environment::ProcessEnvironment;
    use crate::test_support::{ExecutableScript, InheritedPath};

    struct ShimFixture {
        directory: tempfile::TempDir,
    }

    impl ShimFixture {
        const BUILD: &'static str = "/builds/one/ringleader";
        const OTHER_BUILD: &'static str = "/builds/two/ringleader";
        const FOLDER: &'static str = "/run/ringleader/bin/0123456789abcdef";
        const PINNED_TMUX: &'static str = "/nix/store/example-tmux/bin/tmux";
        const PINNED_GIT: &'static str = "/nix/store/example-git/bin/git";
        const OTHER_TMUX: &'static str = "/nix/store/other-tmux/bin/tmux";
        const ENVIRONMENT_PRINTER: &'static str = "#!/bin/sh\nenv\n";
        const ARGUMENT_PRINTER: &'static str = "#!/bin/sh\nprintf '%s\\n' \"$@\"\n";

        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn root(&self) -> PathBuf {
            self.directory.path().join("bin")
        }

        fn shim(&self, executable: &str) -> RingleaderShim {
            RingleaderShim::new(&self.root(), Path::new(executable), &[])
        }

        fn pinning(&self, executable: &Path, tmux: &str) -> RingleaderShim {
            let launch = ProcessEnvironment::new(
                vec![
                    Self::pair("RINGLEADER_TMUX", tmux),
                    Self::pair("RINGLEADER_GIT", Self::PINNED_GIT),
                ],
                None,
            );
            let (programs, _) = launch.split_program_paths();
            RingleaderShim::new(&self.root(), executable, &programs.assignments())
        }

        fn launch_environment() -> ProcessEnvironment {
            ProcessEnvironment::new(
                vec![
                    Self::pair("RINGLEADER_TMUX", Self::PINNED_TMUX),
                    Self::pair("RINGLEADER_GIT", Self::PINNED_GIT),
                    (OsString::from("PATH"), InheritedPath::value()),
                ],
                None,
            )
        }

        fn stand_in_build(&self, body: &str) -> PathBuf {
            let build = self.directory.path().join("build");
            ExecutableScript::write(&build, body);
            build
        }

        fn run(folder: &Path, arguments: &[&str], environment: &ProcessEnvironment) -> String {
            let output = Command::new("sh")
                .arg(folder.join("ringleader"))
                .args(arguments)
                .env_clear()
                .envs(environment.variables().iter().cloned())
                .output()
                .unwrap();
            String::from_utf8(output.stdout).unwrap()
        }

        fn environment_printed_by(output: &str) -> ProcessEnvironment {
            let variables = output
                .lines()
                .filter_map(|line| line.split_once('='))
                .map(|(name, value)| Self::pair(name, value))
                .collect();
            ProcessEnvironment::new(variables, None)
        }

        fn program_of(folder: &Path) -> Vec<u8> {
            fs::read(folder.join("ringleader")).unwrap()
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
    fn an_agent_running_the_shim_gets_the_programs_pinned_at_launch() {
        let fixture = ShimFixture::new();
        let (programs, agent_environment) = ShimFixture::launch_environment().split_program_paths();
        let build = fixture.stand_in_build(ShimFixture::ENVIRONMENT_PRINTER);
        let folder = RingleaderShim::new(&fixture.root(), &build, &programs.assignments())
            .publish()
            .unwrap();

        let printed = ShimFixture::run(&folder, &[], &agent_environment);

        let (used, _) = ShimFixture::environment_printed_by(&printed).split_program_paths();
        assert_eq!(
            (used.tmux(), used.git()),
            (
                OsStr::new(ShimFixture::PINNED_TMUX),
                OsStr::new(ShimFixture::PINNED_GIT)
            )
        );
    }

    #[test]
    fn the_shim_hands_its_arguments_to_the_launching_build() {
        let fixture = ShimFixture::new();
        let build = fixture.stand_in_build(ShimFixture::ARGUMENT_PRINTER);
        let folder = RingleaderShim::new(&fixture.root(), &build, &[])
            .publish()
            .unwrap();

        let printed = ShimFixture::run(
            &folder,
            &["new", "fix it's auth"],
            &ShimFixture::launch_environment(),
        );

        assert_eq!(printed, "new\nfix it's auth\n");
    }

    #[test]
    fn a_published_shim_is_executable() {
        let fixture = ShimFixture::new();

        let folder = fixture.shim(ShimFixture::BUILD).publish().unwrap();

        let mode = fs::metadata(folder.join("ringleader"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    #[test]
    fn a_published_shim_holds_nothing_but_the_program() {
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
    fn a_publish_restores_a_changed_program() {
        let fixture = ShimFixture::new();
        let folder = fixture.shim(ShimFixture::BUILD).publish().unwrap();
        let published = ShimFixture::program_of(&folder);
        fs::write(folder.join("ringleader"), "changed").unwrap();

        fixture.shim(ShimFixture::BUILD).publish().unwrap();

        assert_eq!(ShimFixture::program_of(&folder), published);
    }

    #[test]
    fn two_builds_get_two_folders() {
        let fixture = ShimFixture::new();
        let first = fixture.shim(ShimFixture::BUILD).publish().unwrap();
        let published = ShimFixture::program_of(&first);

        let second = fixture.shim(ShimFixture::OTHER_BUILD).publish().unwrap();

        assert_ne!(first, second);
        assert_eq!(ShimFixture::program_of(&first), published);
    }

    #[test]
    fn one_build_pinning_other_programs_gets_another_folder() {
        let fixture = ShimFixture::new();
        let build = Path::new(ShimFixture::BUILD);
        let first = fixture
            .pinning(build, ShimFixture::PINNED_TMUX)
            .publish()
            .unwrap();

        let second = fixture
            .pinning(build, ShimFixture::OTHER_TMUX)
            .publish()
            .unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn a_build_path_that_is_not_utf8_publishes_no_shim() {
        let fixture = ShimFixture::new();
        let build = Path::new(OsStr::from_bytes(b"/builds/\xff/ringleader"));

        let published = RingleaderShim::new(&fixture.root(), build, &[]).publish();

        assert!(published.is_err());
    }

    #[test]
    fn a_build_path_that_is_not_absolute_publishes_no_shim() {
        let fixture = ShimFixture::new();

        let published = fixture.shim("ringleader").publish();

        assert!(published.is_err());
    }

    #[test]
    fn a_publish_under_a_file_fails() {
        let fixture = ShimFixture::new();
        let blocker = fixture.directory.path().join("blocker");
        fs::write(&blocker, "").unwrap();

        let published =
            RingleaderShim::new(&blocker.join("bin"), Path::new(ShimFixture::BUILD), &[]).publish();

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

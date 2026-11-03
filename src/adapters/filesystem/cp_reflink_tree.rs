use std::path::Path;
use std::process::Command;

use crate::adapters::child_process::bounded_command::Call;
use crate::adapters::child_process::deadlines::CallKind;
use crate::adapters::{BoundedCommand, Deadlines};

pub struct CpReflinkTree {
    bounded: BoundedCommand,
}

impl CpReflinkTree {
    const PROGRAM: &'static str = "cp";
    const SUBCOMMAND: &'static str = "cp --reflink";
    const CLONE_PRESERVING_BUILD_TIMES: [&'static str; 3] =
        ["-R", "--reflink=always", "--preserve=mode,timestamps"];

    pub fn new() -> Self {
        Self {
            bounded: BoundedCommand::new(Deadlines::standard()),
        }
    }

    pub fn clone_tree(&self, source: &Path, destination: &Path) -> Result<(), String> {
        let mut command = Command::new(Self::PROGRAM);
        command
            .args(Self::CLONE_PRESERVING_BUILD_TIMES)
            .arg(source)
            .arg(destination);
        let call = Call::new(Self::SUBCOMMAND, CallKind::BuildCacheCopy);
        let output = self
            .bounded
            .run(command, &call)
            .map_err(|error| error.to_string())?;
        match output.succeeded() {
            true => Ok(()),
            false => Err(Self::first_failure(&output.stderr_text())),
        }
    }

    fn first_failure(stderr: &str) -> String {
        let mut failures = stderr.lines();
        let first = failures.next().unwrap_or_default().to_string();
        match failures.count() {
            0 => first,
            more => format!("{first}, and {more} more failures"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, Permissions};
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use super::*;

    struct TreeFixture {
        directory: tempfile::TempDir,
    }

    impl TreeFixture {
        const UNREADABLE: u32 = 0o000;

        fn with_unreadable_outputs(names: &[&str]) -> Self {
            let fixture = Self {
                directory: tempfile::tempdir().unwrap(),
            };
            fs::create_dir_all(fixture.source().join("debug")).unwrap();
            for name in names {
                let output = fixture.source().join("debug").join(name);
                fs::write(&output, "built").unwrap();
                fs::set_permissions(&output, Permissions::from_mode(Self::UNREADABLE)).unwrap();
            }
            fixture
        }

        fn source(&self) -> PathBuf {
            self.directory.path().join("target")
        }

        fn destination(&self) -> PathBuf {
            self.directory.path().join("clone")
        }

        fn clone_tree(&self) -> Result<(), String> {
            CpReflinkTree::new().clone_tree(&self.source(), &self.destination())
        }
    }

    #[test]
    fn a_failure_on_many_files_is_reported_by_its_first_line_and_a_count() {
        let fixture = TreeFixture::with_unreadable_outputs(&["first", "second", "third"]);

        let detail = fixture.clone_tree().unwrap_err();

        assert!(detail.starts_with("cp: "), "{detail}");
        assert!(detail.ends_with(", and 2 more failures"), "{detail}");
    }
}

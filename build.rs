use std::path::{Path, PathBuf};
use std::process::Command;

struct GitVersion {
    package_directory: PathBuf,
}

impl GitVersion {
    const VARIABLE: &str = "RINGLEADER_VERSION";
    const TAG_PATTERN: &str = "v*";

    fn new(package_directory: &Path) -> Self {
        Self {
            package_directory: package_directory.to_path_buf(),
        }
    }

    fn publish(&self) {
        self.watch_inputs();
        if let Some(version) = self.describe() {
            println!("cargo::rustc-env={}={version}", Self::VARIABLE);
        }
    }

    fn watch_inputs(&self) {
        let package_inputs = [PathBuf::from("src"), PathBuf::from("Cargo.toml")];
        for input in package_inputs.into_iter().chain(self.git_inputs()) {
            println!("cargo::rerun-if-changed={}", input.display());
        }
    }

    fn git_inputs(&self) -> Vec<PathBuf> {
        let worktree_inputs = self.git_files("--git-dir", &["HEAD", "index"]);
        let repository_inputs = self.git_files("--git-common-dir", &["refs", "packed-refs"]);
        worktree_inputs
            .into_iter()
            .chain(repository_inputs)
            .collect()
    }

    fn git_files(&self, directory_flag: &str, names: &[&str]) -> Vec<PathBuf> {
        self.git_path(directory_flag)
            .map(|directory| names.iter().map(|name| directory.join(name)).collect())
            .unwrap_or_default()
    }

    fn describe(&self) -> Option<String> {
        self.git(&[
            "describe",
            "--tags",
            "--match",
            Self::TAG_PATTERN,
            "--dirty",
        ])
    }

    fn git_path(&self, directory_flag: &str) -> Option<PathBuf> {
        self.git(&["rev-parse", "--path-format=absolute", directory_flag])
            .map(PathBuf::from)
    }

    fn git(&self, arguments: &[&str]) -> Option<String> {
        let output = Command::new("git")
            .args(arguments)
            .current_dir(&self.package_directory)
            .env("GIT_CEILING_DIRECTORIES", self.package_directory.parent()?)
            .output()
            .ok()?;
        let stdout = String::from_utf8(output.stdout).ok()?;
        output.status.success().then(|| stdout.trim().to_string())
    }
}

fn main() {
    GitVersion::new(Path::new(env!("CARGO_MANIFEST_DIR"))).publish();
}

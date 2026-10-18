use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use super::{Deadline, ExecutableScript, InheritedPath};
use crate::adapters::host::process_environment::ProcessEnvironment;

pub struct RecordingPrograms {
    directory: tempfile::TempDir,
    prelude: String,
}

impl RecordingPrograms {
    pub const NAME_PLACEHOLDER: &'static str = "{name}";
    pub const FAILURE: &'static str = "exit 3";
    const SUCCESS: &'static str = "exit 0";
    const NOT_EXECUTABLE: u32 = 0o644;
    const LINGERING_SECONDS: u32 = 30;
    const RECORDING: &'static str = r#"#!/bin/sh
{prelude}
record='{record}'
'{cat}' /proc/$$/environ > "$record/environ"
read -r stat < /proc/$$/stat
echo "$stat" > "$record/stat"
pwd -P > "$record/cwd"
: > "$record/arguments"
for argument in "$@"; do
  printf '%s\n' "$argument" >> "$record/arguments"
done
for descriptor in 0 1 2; do
  if [ /proc/$$/fd/$descriptor -ef /dev/null ]; then
    echo null >> "$record/streams"
  else
    echo terminal >> "$record/streams"
  fi
done
echo $$ > "$record/pid"
{ending}
"#;

    pub fn new() -> Self {
        let programs = Self {
            directory: tempfile::tempdir().unwrap(),
            prelude: String::new(),
        };
        fs::create_dir(programs.program_directory()).unwrap();
        programs
    }

    pub fn with_prelude(self, prelude: &str) -> Self {
        Self {
            prelude: prelude.to_string(),
            ..self
        }
    }

    pub fn make_directory(&self, name: &str) -> PathBuf {
        let directory = self.directory.path().join(name);
        fs::create_dir(&directory).unwrap();
        directory
    }

    pub fn program_directory(&self) -> PathBuf {
        self.directory.path().join("bin")
    }

    pub fn install(&self, name: &str) -> PathBuf {
        self.install_script(name, Self::SUCCESS)
    }

    pub fn install_failing(&self, name: &str) -> PathBuf {
        self.install_script(name, Self::FAILURE)
    }

    pub fn install_unexecutable(&self, name: &str) -> PathBuf {
        let program = self.install_script(name, Self::SUCCESS);
        fs::set_permissions(&program, fs::Permissions::from_mode(Self::NOT_EXECUTABLE)).unwrap();
        program
    }

    pub fn install_lingering(&self, name: &str) -> PathBuf {
        let sleep = Self::tool("sleep").display().to_string();
        let ending = format!("exec '{sleep}' {}", Self::LINGERING_SECONDS);
        self.install_script(name, &ending)
    }

    pub fn environment(&self, variables: &[(&str, &str)]) -> ProcessEnvironment {
        let search_path = ("PATH".into(), self.program_directory().into_os_string());
        let variables = variables
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)));
        ProcessEnvironment::new(
            std::iter::once(search_path).chain(variables).collect(),
            None,
        )
    }

    pub fn pid(&self, name: &str) -> Option<u32> {
        let mut pid = None;
        Deadline::for_background_work().wait_until(|| {
            pid = self.recorded(name, "pid").trim().parse().ok();
            pid.is_some()
        });
        pid
    }

    pub fn never_ran(&self, name: &str) -> bool {
        self.recorded(name, "pid").is_empty()
    }

    pub fn ran_in(&self, name: &str) -> Option<PathBuf> {
        self.pid(name)?;
        Some(PathBuf::from(self.recorded(name, "cwd").trim()))
    }

    pub fn environment_of(&self, name: &str) -> Vec<String> {
        let mut variables: Vec<String> = self
            .recorded_once_started(name, "environ")
            .split('\0')
            .filter(|variable| !variable.is_empty())
            .map(str::to_string)
            .collect();
        variables.sort();
        variables
    }

    pub fn streams_of(&self, name: &str) -> String {
        self.recorded_once_started(name, "streams")
    }

    pub fn arguments_of(&self, name: &str) -> String {
        self.recorded_once_started(name, "arguments")
    }

    pub fn session_of(&self, name: &str) -> Option<u32> {
        self.pid(name)?;
        let stat = self.recorded(name, "stat");
        let (_, fields) = stat.rsplit_once(')')?;
        fields.split_whitespace().nth(3)?.parse().ok()
    }

    fn install_script(&self, name: &str, ending: &str) -> PathBuf {
        let record = self.record_directory(name);
        fs::create_dir_all(&record).unwrap();
        let script = Self::RECORDING
            .replace("{prelude}", &self.prelude)
            .replace(Self::NAME_PLACEHOLDER, name)
            .replace("{record}", &record.to_string_lossy())
            .replace("{cat}", &Self::tool("cat").to_string_lossy())
            .replace("{ending}", ending);
        let program = self.program_directory().join(name);
        ExecutableScript::write(&program, &script);
        program
    }

    fn tool(name: &str) -> PathBuf {
        InheritedPath::program(name).expect("the test PATH holds the tool")
    }

    fn recorded_once_started(&self, name: &str, file: &str) -> String {
        self.pid(name);
        self.recorded(name, file)
    }

    fn recorded(&self, name: &str, file: &str) -> String {
        fs::read_to_string(self.record_directory(name).join(file)).unwrap_or_default()
    }

    fn record_directory(&self, name: &str) -> PathBuf {
        self.directory.path().join("record").join(name)
    }
}

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct ExecutableScript;

impl ExecutableScript {
    const MODE: u32 = 0o755;

    pub fn write(path: &Path, body: &str) {
        Self::write_from_a_child_process(path, body);
        fs::set_permissions(path, fs::Permissions::from_mode(Self::MODE)).unwrap();
    }

    fn write_from_a_child_process(path: &Path, body: &str) {
        let mut tee = Command::new("tee")
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        tee.stdin
            .as_mut()
            .unwrap()
            .write_all(body.as_bytes())
            .unwrap();
        assert!(
            tee.wait().unwrap().success(),
            "tee could not write {}",
            path.display()
        );
    }
}

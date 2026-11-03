use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::adapters::host::errno::Errno;

pub struct ClonefileTree;

impl ClonefileTree {
    const CLONE_FLAGS: u32 = 0;

    pub fn new() -> Self {
        Self
    }

    pub fn clone_tree(&self, source: &Path, destination: &Path) -> Result<(), String> {
        let source = Self::c_path(source)?;
        let destination = Self::c_path(destination)?;
        // SAFETY: both pointers come from CStrings that outlive the call, and clonefile only
        // reads them. clonefile copies a whole directory tree with its file times, and fails
        // instead of falling back to a full copy on a volume without clones.
        let cloned =
            unsafe { libc::clonefile(source.as_ptr(), destination.as_ptr(), Self::CLONE_FLAGS) };
        Errno::result(cloned)
            .map(drop)
            .map_err(|error| error.to_string())
    }

    fn c_path(path: &Path) -> Result<CString, String> {
        CString::new(path.as_os_str().as_bytes()).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::time::{Duration, SystemTime};

    use super::*;

    #[test]
    fn a_cloned_tree_keeps_its_contents_and_file_times() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("target");
        fs::create_dir_all(source.join("debug")).unwrap();
        fs::write(source.join("debug/output"), "built").unwrap();
        let built = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        File::open(source.join("debug/output"))
            .unwrap()
            .set_modified(built)
            .unwrap();
        let destination = directory.path().join("clone");

        ClonefileTree::new()
            .clone_tree(&source, &destination)
            .unwrap();

        let copied = destination.join("debug/output");
        assert_eq!(fs::read_to_string(&copied).unwrap(), "built");
        assert_eq!(fs::metadata(&copied).unwrap().modified().unwrap(), built);
    }
}

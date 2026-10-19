use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::mount_source::MountSource;
use super::mount_table::{Mount, MountTable};

pub struct MountinfoFile {
    path: PathBuf,
}

impl MountinfoFile {
    const OWN_MOUNTINFO: &'static str = "/proc/self/mountinfo";
    const OPTIONAL_FIELDS_END: &'static str = "-";
    const MOUNT_POINT_FIELD: usize = 4;
    const REMOTE_TYPES: [&'static str; 7] = ["nfs", "nfs4", "cifs", "smb3", "9p", "ceph", "autofs"];
    const FUSE_PREFIX: &'static str = "fuse";

    pub fn of_this_process() -> Self {
        Self::at(Path::new(Self::OWN_MOUNTINFO))
    }

    pub fn at(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    fn mount(line: &str) -> Option<Mount> {
        let fields: Vec<&str> = line.split(' ').collect();
        let separator = fields
            .iter()
            .position(|field| *field == Self::OPTIONAL_FIELDS_END)?;
        let filesystem = fields.get(separator + 1)?;
        let point = fields.get(Self::MOUNT_POINT_FIELD)?;
        Some(Mount::new(
            PathBuf::from(Self::decode(point)),
            Self::is_remote(filesystem),
        ))
    }

    fn is_remote(filesystem: &str) -> bool {
        filesystem.starts_with(Self::FUSE_PREFIX) || Self::REMOTE_TYPES.contains(&filesystem)
    }

    fn decode(escaped: &str) -> String {
        let bytes = escaped.as_bytes();
        let mut decoded = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            let octal = bytes.get(index + 1..index + 4).and_then(Self::octal_value);
            match (bytes[index], octal) {
                (b'\\', Some(value)) => {
                    decoded.push(value);
                    index += 4;
                }
                (byte, _) => {
                    decoded.push(byte);
                    index += 1;
                }
            }
        }
        String::from_utf8_lossy(&decoded).into_owned()
    }

    fn octal_value(digits: &[u8]) -> Option<u8> {
        let text = std::str::from_utf8(digits).ok()?;
        u8::from_str_radix(text, 8).ok()
    }
}

impl MountSource for MountinfoFile {
    fn mount_table(&self) -> io::Result<MountTable> {
        let bytes = fs::read(&self.path)?;
        let text = String::from_utf8_lossy(&bytes);
        Ok(MountTable::new(
            text.lines().filter_map(Self::mount).collect(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "22 1 0:21 / /proc rw - proc proc rw\n\
        41 30 0:40 / /home/user/remote\\040files rw - fuse.sshfs host: rw\n\
        42 30 0:41 / /mnt/share rw shared:7 - nfs4 server:/export rw\n\
        43 30 254:1 / /home rw - ext4 /dev/vda1 rw\n";

    struct MountinfoFixture {
        _directory: tempfile::TempDir,
        file: MountinfoFile,
    }

    impl MountinfoFixture {
        fn holding(bytes: &[u8]) -> Self {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("mountinfo");
            fs::write(&path, bytes).unwrap();
            Self {
                file: MountinfoFile::at(&path),
                _directory: directory,
            }
        }

        fn table() -> MountTable {
            Self::holding(TABLE.as_bytes()).file.mount_table().unwrap()
        }
    }

    #[test]
    fn a_fuse_mount_with_an_escaped_space_is_remote() {
        assert!(MountinfoFixture::table().is_remote_mount(Path::new("/home/user/remote files")));
    }

    #[test]
    fn a_network_mount_after_optional_fields_is_remote() {
        assert!(MountinfoFixture::table().is_remote_mount(Path::new("/mnt/share")));
    }

    #[test]
    fn a_local_mount_is_not_remote() {
        assert!(!MountinfoFixture::table().is_remote_mount(Path::new("/home")));
    }

    #[test]
    fn a_mount_below_a_directory_is_within_it() {
        assert!(MountinfoFixture::table().has_mount_within(Path::new("/home/user")));
    }

    #[test]
    fn a_missing_file_is_an_error() {
        let file = MountinfoFile::at(Path::new("/nonexistent/mountinfo"));

        assert!(file.mount_table().is_err());
    }

    #[test]
    fn a_line_that_is_not_utf8_hides_no_other_mount() {
        let mut bytes = b"40 30 0:39 / /home/user/donn\xe9es rw - fuse.sshfs host: rw\n".to_vec();
        bytes.extend_from_slice(TABLE.as_bytes());

        let table = MountinfoFixture::holding(&bytes)
            .file
            .mount_table()
            .unwrap();

        assert!(table.is_remote_mount(Path::new("/mnt/share")));
    }
}

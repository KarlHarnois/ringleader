use std::fs;
use std::path::{Path, PathBuf};

pub struct MountTable {
    mounts: Vec<Mount>,
}

struct Mount {
    filesystem: String,
    point: PathBuf,
}

impl Mount {
    const REMOTE_TYPES: [&'static str; 7] = ["nfs", "nfs4", "cifs", "smb3", "9p", "ceph", "autofs"];
    const FUSE_PREFIX: &'static str = "fuse";

    fn is_remote(&self) -> bool {
        self.filesystem.starts_with(Self::FUSE_PREFIX)
            || Self::REMOTE_TYPES.contains(&self.filesystem.as_str())
    }
}

impl MountTable {
    pub const PROCESS_TABLE: &'static str = "/proc/self/mountinfo";
    const OPTIONAL_FIELDS_END: &'static str = "-";
    const MOUNT_POINT_FIELD: usize = 4;

    pub fn read(path: &Path) -> Self {
        let text = fs::read_to_string(path).unwrap_or_default();
        Self {
            mounts: text.lines().filter_map(Self::mount).collect(),
        }
    }

    pub fn is_remote_mount(&self, path: &Path) -> bool {
        self.mounts
            .iter()
            .any(|mount| mount.point == path && mount.is_remote())
    }

    pub fn has_mount_within(&self, directory: &Path) -> bool {
        self.mounts
            .iter()
            .any(|mount| mount.point.starts_with(directory))
    }

    fn mount(line: &str) -> Option<Mount> {
        let fields: Vec<&str> = line.split(' ').collect();
        let separator = fields
            .iter()
            .position(|field| *field == Self::OPTIONAL_FIELDS_END)?;
        let filesystem = fields.get(separator + 1)?;
        let point = fields.get(Self::MOUNT_POINT_FIELD)?;
        Some(Mount {
            filesystem: filesystem.to_string(),
            point: PathBuf::from(Self::decode(point)),
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "22 1 0:21 / /proc rw - proc proc rw\n\
        41 30 0:40 / /home/user/remote\\040files rw - fuse.sshfs host: rw\n\
        42 30 0:41 / /mnt/share rw shared:7 - nfs4 server:/export rw\n\
        43 30 254:1 / /home rw - ext4 /dev/vda1 rw\n";

    struct TableFixture;

    impl TableFixture {
        fn table() -> MountTable {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("mountinfo");
            fs::write(&path, TABLE).unwrap();
            MountTable::read(&path)
        }
    }

    #[test]
    fn a_fuse_mount_with_an_escaped_space_is_remote() {
        assert!(TableFixture::table().is_remote_mount(Path::new("/home/user/remote files")));
    }

    #[test]
    fn a_network_mount_after_optional_fields_is_remote() {
        assert!(TableFixture::table().is_remote_mount(Path::new("/mnt/share")));
    }

    #[test]
    fn a_local_mount_is_not_remote() {
        assert!(!TableFixture::table().is_remote_mount(Path::new("/home")));
    }

    #[test]
    fn a_mount_below_a_directory_is_within_it() {
        assert!(TableFixture::table().has_mount_within(Path::new("/home/user")));
    }

    #[test]
    fn a_mount_whose_name_only_starts_like_a_directory_is_not_within_it() {
        assert!(!TableFixture::table().has_mount_within(Path::new("/home/user/remote")));
    }

    #[test]
    fn an_unreadable_table_skips_nothing() {
        let table = MountTable::read(Path::new("/nonexistent/mountinfo"));

        assert!(!table.is_remote_mount(Path::new("/mnt/share")));
    }
}

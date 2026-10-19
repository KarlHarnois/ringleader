use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct MountTable {
    mounts: Vec<Mount>,
}

pub struct Mount {
    point: PathBuf,
    remote: bool,
}

impl Mount {
    pub fn new(point: PathBuf, remote: bool) -> Self {
        Self { point, remote }
    }
}

impl MountTable {
    pub fn new(mounts: Vec<Mount>) -> Self {
        Self { mounts }
    }

    pub fn is_remote_mount(&self, path: &Path) -> bool {
        self.mounts
            .iter()
            .any(|mount| mount.point == path && mount.remote)
    }

    pub fn has_mount_within(&self, directory: &Path) -> bool {
        self.mounts
            .iter()
            .any(|mount| mount.point.starts_with(directory))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TableFixture;

    impl TableFixture {
        fn table() -> MountTable {
            MountTable::new(vec![
                Mount::new(PathBuf::from("/home/user/remote files"), true),
                Mount::new(PathBuf::from("/home"), false),
            ])
        }
    }

    #[test]
    fn a_remote_mount_is_remote_at_its_point() {
        assert!(TableFixture::table().is_remote_mount(Path::new("/home/user/remote files")));
    }

    #[test]
    fn a_local_mount_is_not_remote() {
        assert!(!TableFixture::table().is_remote_mount(Path::new("/home")));
    }

    #[test]
    fn a_path_below_a_remote_mount_is_not_its_point() {
        assert!(!TableFixture::table().is_remote_mount(Path::new("/home/user/remote files/docs")));
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
    fn an_empty_table_holds_no_remote_mount() {
        assert!(!MountTable::default().is_remote_mount(Path::new("/home")));
    }
}

use std::io;

use super::mount_table::MountTable;

pub trait MountSource: Send + Sync {
    fn mount_table(&self) -> io::Result<MountTable>;
}

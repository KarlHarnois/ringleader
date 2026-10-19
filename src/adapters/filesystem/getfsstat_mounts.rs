use std::io;
use std::ptr;

use super::mount_source::MountSource;
use super::mount_table::{Mount, MountTable};
use crate::adapters::host::errno::Errno;
use crate::adapters::host::nul_terminated::NulTerminated;

pub struct GetfsstatMounts;

impl GetfsstatMounts {
    const MOUNT_HEADROOM: usize = 16;
    const LOCAL: u32 = libc::MNT_LOCAL as u32;

    pub fn of_this_process() -> Self {
        Self
    }

    fn count() -> io::Result<usize> {
        // SAFETY: a null buffer asks only for the count, so getfsstat writes nothing.
        let count = unsafe { libc::getfsstat(ptr::null_mut(), 0, libc::MNT_NOWAIT) };
        Errno::result(count).map(|count| usize::try_from(count).unwrap_or_default())
    }

    fn filesystems(count: usize) -> io::Result<Vec<libc::statfs>> {
        let mut filesystems = vec![libc::statfs::default(); count + Self::MOUNT_HEADROOM];
        let size = libc::c_int::try_from(size_of_val(filesystems.as_slice())).unwrap_or_default();
        // SAFETY: getfsstat writes at most `size` bytes, the size of the buffer `filesystems`
        // owns, so the scanner never shares the static storage getmntinfo would return.
        let listed = unsafe { libc::getfsstat(filesystems.as_mut_ptr(), size, libc::MNT_NOWAIT) };
        Errno::result(listed).map(|listed| {
            filesystems.truncate(usize::try_from(listed).unwrap_or_default());
            filesystems
        })
    }

    fn mount(filesystem: &libc::statfs) -> Option<Mount> {
        let remote = filesystem.f_flags & Self::LOCAL == 0;
        NulTerminated::path(&filesystem.f_mntonname).map(|point| Mount::new(point, remote))
    }
}

impl MountSource for GetfsstatMounts {
    fn mount_table(&self) -> io::Result<MountTable> {
        Self::count()
            .and_then(Self::filesystems)
            .map(|filesystems| {
                MountTable::new(filesystems.iter().filter_map(Self::mount).collect())
            })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn this_process_sees_the_root_mount() {
        let table = GetfsstatMounts::of_this_process().mount_table().unwrap();

        assert!(table.has_mount_within(Path::new("/")));
    }

    #[test]
    fn the_root_mount_is_local() {
        let table = GetfsstatMounts::of_this_process().mount_table().unwrap();

        assert!(!table.is_remote_mount(Path::new("/")));
    }
}

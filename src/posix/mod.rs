//-
// Copyright (c) 2016, 2017, 2024, Jason Lingle
//
// This file is part of Ensync.
//
// Ensync is free software: you can  redistribute it and/or modify it under the
// terms of  the GNU General Public  License as published by  the Free Software
// Foundation, either version  3 of the License, or (at  your option) any later
// version.
//
// Ensync is distributed  in the hope that  it will be useful,  but WITHOUT ANY
// WARRANTY; without  even the implied  warranty of MERCHANTABILITY  or FITNESS
// FOR  A PARTICULAR  PURPOSE.  See the  GNU General  Public  License for  more
// details.
//
// You should have received a copy of the GNU General Public License along with
// Ensync. If not, see <http://www.gnu.org/licenses/>.

//! The client filesystem replica, shared by Unix and Windows.
//!
//! The bulk of the replica is represented literally on the local filesystem.
//! Some side data (a cache of hashes as well as data to know which directories
//! are still clean) is stored in a SQLite database under a directory owned by
//! the replica. Additionally, the replica uses the directory it owns for
//! temporary files created during the sync process.
//!
//! The private directory should be on the same filesystem as the sync root.
//! This is due to its use as a cache to optimise renames (which can look like
//! a delete followed by a create), which requires being able to rename files
//! to or from their location under the sync root. If the sync crosses a
//! filesystem boundary, this rename optimisation does not occur; renamed files
//! will instead be re-downloaded if the deletion occurs before the create.
//!
//! Platform-specific filesystem operations are mostly in `crate::platform`.
//! On Windows, download staging takes place in the target directory so files
//! inherit its ACLs.
//!
//! - The `dao` submodule manages the SQLite database.
//!
//! - The `replica` submodule implements a `Replica` atop the other submodules.
//!
//! Originally, the intent here was to use the raw POSIX API for interacting
//! with the filesystem due to the Rust API needing us to convert our
//! `OsString`s into `OsStr`s just so it can convert them back into new
//! `OsString`s. However, the fact that Linux *still* uses 32-bit off_t on
//! 32-bit platforms and requires separate 64-suffixed functions and types to
//! deal with that was sufficient inconvenience to use the API provided by
//! Rust.
//!
//! This top-level module also has some miscellaneous functions for working
//! with POSIX filesystems.

use std::fs::{File, FileTimes};
use std::io;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use crate::defs::FileTime;

mod dao;
mod dir;
mod replica;

#[allow(unused_imports)]
pub use self::dir::DirHandle;
pub use self::replica::PosixReplica;

/// Set the atime and mtime of the given file handle to the given time, in
/// seconds.
pub fn set_mtime(file: &File, mtime: FileTime) -> io::Result<()> {
    let delta = Duration::from_secs(mtime.unsigned_abs());
    let time = if mtime < 0 {
        UNIX_EPOCH.checked_sub(delta)
    } else {
        UNIX_EPOCH.checked_add(delta)
    }
    .ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "timestamp out of range")
    })?;
    file.set_times(FileTimes::new().set_accessed(time).set_modified(time))
}

/// Like `set_mtime`, but follows a path (including symbolic links).
pub fn set_mtime_path(
    path: impl AsRef<Path>,
    mtime: FileTime,
) -> io::Result<()> {
    let time = filetime::FileTime::from_unix_time(mtime, 0);
    filetime::set_file_times(path, time, time)
}

#[cfg(all(test, unix))]
mod timestamp_tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, symlink};

    #[test]
    fn pre_epoch_times_on_open_file_and_symlink_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        let link = dir.path().join("link");
        let file = File::create(&path).unwrap();
        symlink(&path, &link).unwrap();
        set_mtime(&file, -10).unwrap();
        let m = file.metadata().unwrap();
        assert_eq!((-10, -10), (m.atime(), m.mtime()));
        set_mtime_path(&link, -20).unwrap();
        let m = file.metadata().unwrap();
        assert_eq!((-20, -20), (m.atime(), m.mtime()));
    }
}

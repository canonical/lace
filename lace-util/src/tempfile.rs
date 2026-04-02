// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.

//! Temporary files and directories via `mkostemp(3)` and `mkdtemp(3)`.
//!
//! Provides [`TempFile`] and [`TempDir`], minimal RAII guards that create
//! temporary files/directories using the standard POSIX calls and remove
//! them on drop.

use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::io::FromRawFd;
use std::path::{Path, PathBuf};

/// Returns a mutable `Vec<u8>` of the form `"$TMPDIR/{prefix}XXXXXX\0"`,
/// or `"{prefix}XXXXXX\0"` if `prefix` is an absolute path.
fn make_template(prefix: &str) -> Vec<u8> {
    let template = std::env::temp_dir().join(format!("{prefix}XXXXXX"));
    let mut buf: Vec<u8> = template.into_os_string().into_vec();
    buf.push(0);
    buf
}

/// A temporary file created via `mkostemp(3)`, removed on drop.
///
/// The file descriptor returned by `mkostemp` is wrapped in a [`File`],
/// accessible via [`TempFile::file`]
pub struct TempFile {
    file: File,
    path: PathBuf,
}

impl TempFile {
    /// Create a new temporary file with a name starting with `prefix`.
    ///
    /// The file is created under [`std::env::temp_dir()`] unless the specified
    /// path is absolute. The six trailing `X` characters required by `mkostemp(3)`
    /// are appended automatically.
    pub fn with_prefix(prefix: &str) -> io::Result<Self> {
        let mut buf = make_template(prefix);

        // SAFETY: `buf` is a valid NUL-terminated C string whose last
        // seven bytes (before the NUL) are "XXXXXX" as required by mkostemp(3).
        // mkostemp replaces the X's in-place and returns an open file descriptor.
        let fd =
            unsafe { libc::mkostemp(buf.as_mut_ptr().cast::<libc::c_char>(), libc::O_CLOEXEC) };

        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        buf.pop(); // drop the NUL terminator

        // SAFETY: `fd` is a valid, open file descriptor owned by us.
        let file = unsafe { File::from_raw_fd(fd) };
        Ok(Self {
            file,
            path: PathBuf::from(OsString::from_vec(buf)),
        })
    }

    /// Return a reference to the underlying [`File`].
    pub fn file(&self) -> &File {
        &self.file
    }

    /// Return the path of the temporary file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A temporary directory created via `mkdtemp(3)`, removed on drop.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Create a new temporary directory with a name starting with `prefix`.
    ///
    /// The directory is created under [`std::env::temp_dir()`] unless the specified
    /// path is absolute. The six trailing `X` characters required by `mkdtemp(3)`
    /// are appended automatically.
    pub fn with_prefix(prefix: &str) -> io::Result<Self> {
        let mut buf = make_template(prefix);

        // SAFETY: `buf` is a valid NUL-terminated C string whose last
        // seven bytes (before the NUL) are "XXXXXX" as required by mkdtemp(3).
        let ret = unsafe { libc::mkdtemp(buf.as_mut_ptr().cast::<libc::c_char>()) };

        if ret.is_null() {
            return Err(io::Error::last_os_error());
        }

        buf.pop(); // drop the NUL terminator
        Ok(Self {
            path: PathBuf::from(OsString::from_vec(buf)),
        })
    }

    /// Return the path of the temporary directory.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

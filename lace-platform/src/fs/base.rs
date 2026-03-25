// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Julian Andres Klode <julian.klode@canonical.com>
//! Block device and filesystem abstraction layer.
//!
//! This module provides a platform-independent filesystem interface.
//! Implementations are provided by platform-specific modules (e.g., efi).

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use crate::Error;

/// Errors that can occur during filesystem operations.
#[derive(Debug)]
pub enum FsError {
    /// Platform I/O error (wraps `DiskError` on BIOS, `uefi::Error` on EFI).
    Io(Error),
    /// Data format or validation error (invalid partition table, bad superblock, etc.)
    Invalid,
    /// File not found
    NotFound,
    /// Not a directory
    NotDirectory,
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FsError::Io(e) => write!(f, "I/O error: {}", e),
            FsError::Invalid => write!(f, "invalid data"),
            FsError::NotFound => write!(f, "not found"),
            FsError::NotDirectory => write!(f, "not a directory"),
        }
    }
}

impl From<Error> for FsError {
    fn from(e: Error) -> Self {
        FsError::Io(e)
    }
}

/// Block device providing both sector-level and byte-level I/O.
///
/// Implementations must provide sector-level I/O via [`read_sectors`],
/// [`sector_size`], and [`sector_count`]. Byte-level I/O ([`read_bytes`],
/// [`block_size`], [`size`]) has default implementations that translate
/// byte offsets into sector reads. Platforms with native byte-level access
/// (e.g. UEFI DiskIO) can override the byte-level methods directly.
///
/// The caller provides a buffer that can be anywhere in memory; the
/// implementation is responsible for any internal bouncing (e.g. BIOS
/// low-memory requirements).
pub trait BlockDevice {
    /// Read `count` sectors starting at `lba` into `buf`.
    ///
    /// `buf` must be at least `count * sector_size()` bytes.
    fn read_sectors(&mut self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), FsError>;

    /// Sector size in bytes.
    fn sector_size(&self) -> u32;

    /// Total number of sectors on the device.
    fn sector_count(&self) -> u64;

    /// Read bytes from the device starting at the given byte offset.
    ///
    /// The default implementation translates byte offsets into sector-aligned
    /// reads via [`read_sectors`](Self::read_sectors). Platforms with native
    /// byte-level access can override this.
    fn read_bytes(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), FsError> {
        let ss = self.sector_size() as u64;
        if ss == 0 || buf.is_empty() {
            return if ss == 0 {
                Err(FsError::Invalid)
            } else {
                Ok(())
            };
        }

        let ss_usize = ss as usize;
        let mut pos = 0usize;
        let mut off = offset;
        let total = buf.len();

        // 1) Partial head sector
        let head_offset = (off % ss) as usize;
        if head_offset != 0 {
            let lba = off / ss;
            let avail = ss_usize - head_offset;
            let n = core::cmp::min(total, avail);
            let mut sector_buf = vec![0u8; ss_usize];
            self.read_sectors(lba, 1, &mut sector_buf)?;
            buf[..n].copy_from_slice(&sector_buf[head_offset..head_offset + n]);
            pos += n;
            off += n as u64;
        }

        // 2) Aligned middle sectors — batch into one read_sectors call
        let remaining = total - pos;
        let full_sectors = remaining / ss_usize;
        if full_sectors > 0 {
            let lba = off / ss;
            let byte_count = full_sectors * ss_usize;
            self.read_sectors(lba, full_sectors as u32, &mut buf[pos..pos + byte_count])?;
            pos += byte_count;
            off += byte_count as u64;
        }

        // 3) Partial tail sector
        let tail = total - pos;
        if tail > 0 {
            let lba = off / ss;
            let mut sector_buf = vec![0u8; ss_usize];
            self.read_sectors(lba, 1, &mut sector_buf)?;
            buf[pos..pos + tail].copy_from_slice(&sector_buf[..tail]);
        }

        Ok(())
    }

    /// Get the block size of the underlying device.
    ///
    /// Defaults to [`sector_size`](Self::sector_size).
    fn block_size(&self) -> u32 {
        self.sector_size()
    }

    /// Get the total size of the device in bytes.
    ///
    /// Defaults to `sector_count() * sector_size()`.
    fn size(&self) -> u64 {
        self.sector_count() * self.sector_size() as u64
    }
}

/// Represents a filesystem.
pub trait Filesystem {
    /// Open a file for reading.
    fn open_file(&mut self, path: &str) -> Result<Box<dyn File>, FsError>;

    /// Check if a file exists.
    fn file_exists(&mut self, path: &str) -> bool;

    /// Read directory entries.
    fn read_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, FsError>;
}

/// Represents an open file.
pub trait File {
    /// Read data from the file.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, FsError>;

    /// Read the entire file into a vector.
    fn read_to_end(&mut self) -> Result<Vec<u8>, FsError>;

    /// Get the size of the file.
    fn size(&mut self) -> u64;
}

/// Directory entry.
#[derive(Debug)]
pub struct DirEntry {
    /// Name of the entry
    pub name: String,
    /// Whether this is a directory
    pub is_dir: bool,
}

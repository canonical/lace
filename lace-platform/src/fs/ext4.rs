// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.

extern crate alloc;

use super::base::{DirEntry, File, Filesystem, FsError};
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

/// ext4 filesystem implementation using ext4-view.
pub struct Ext4Filesystem {
    fs: ext4_view::Ext4,
}

impl Ext4Filesystem {
    /// Try to create a new ext4 filesystem from a block device.
    pub fn new(block_dev: Box<dyn crate::fs::base::BlockDevice>) -> Result<Self, FsError> {
        crate::debugln!("[FS] Loading ext4 filesystem from block device");

        struct Adapter {
            dev: Box<dyn crate::fs::base::BlockDevice>,
        }

        impl ext4_view::Ext4Read for Adapter {
            fn read(
                &mut self,
                start_byte: u64,
                dst: &mut [u8],
            ) -> Result<(), Box<dyn core::error::Error + Send + Sync>> {
                self.dev
                    .read_bytes(start_byte, dst)
                    .map_err(|_| Box::<dyn core::error::Error + Send + Sync>::from("I/O"))?;
                Ok(())
            }
        }

        let adapter = Adapter { dev: block_dev };
        let fs = ext4_view::Ext4::load(Box::new(adapter)).map_err(|e| {
            crate::debugln!("[FS] Failed to load ext4 filesystem: {:?}", e);
            FsError::Invalid
        })?;

        crate::debugln!("[FS] Successfully loaded ext4 filesystem");
        Ok(Self { fs })
    }
}

impl Filesystem for Ext4Filesystem {
    fn open_file(&mut self, path: &str) -> Result<Box<dyn File>, FsError> {
        // Ensure path starts with /
        let full_path = if path.starts_with('/') {
            path
        } else {
            // ext4-view requires absolute paths
            crate::debugln!(
                "[FS] ext4: open_file failed - path is not absolute: {}",
                path
            );
            return Err(FsError::NotFound);
        };

        crate::debugln!("[FS] ext4: opening file: {}", full_path);
        let file = self.fs.open(full_path).map_err(|e| {
            crate::debugln!("[FS] ext4: failed to open {}: {:?}", full_path, e);
            match e {
                ext4_view::Ext4Error::NotFound => FsError::NotFound,
                ext4_view::Ext4Error::IsADirectory => FsError::NotDirectory,
                _ => FsError::Invalid,
            }
        })?;

        crate::debugln!("[FS] ext4: successfully opened file: {}", full_path);
        Ok(Box::new(Ext4File { file }))
    }

    fn file_exists(&mut self, path: &str) -> bool {
        let full_path = if path.starts_with('/') {
            path
        } else {
            crate::debugln!("[FS] ext4: file_exists - path not absolute: {}", path);
            return false;
        };

        let exists = self.fs.exists(full_path).unwrap_or(false);
        crate::debugln!("[FS] ext4: file_exists({}) = {}", full_path, exists);
        exists
    }

    fn read_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, FsError> {
        let full_path = if path.starts_with('/') {
            path
        } else {
            return Err(FsError::NotFound);
        };

        let mut results = Vec::new();

        for entry_result in self.fs.read_dir(full_path).map_err(|e| match e {
            ext4_view::Ext4Error::NotFound => FsError::NotFound,
            _ => FsError::Invalid,
        })? {
            let entry = entry_result.map_err(|_| FsError::Invalid)?;

            let name_str = entry.file_name().as_str().map_err(|_| FsError::Invalid)?;
            let name = alloc::string::String::from(name_str);

            // Skip . and ..
            if name == "." || name == ".." {
                continue;
            }

            let file_type = entry.file_type().map_err(|_| FsError::Invalid)?;
            let is_dir = file_type.is_dir();

            results.push(DirEntry { name, is_dir });
        }

        Ok(results)
    }
}

/// ext4 file handle using ext4-view.
struct Ext4File {
    file: ext4_view::File,
}

impl File for Ext4File {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, FsError> {
        self.file.read_bytes(buf).map_err(|_| FsError::Invalid)
    }

    fn read_to_end(&mut self) -> Result<Vec<u8>, FsError> {
        crate::debugln!("[FS] ext4: read_to_end called");
        let metadata = self.file.metadata();
        let size = metadata.len() as usize;
        let mut buf = vec![0u8; size];

        let mut total_read = 0;
        while total_read < size {
            let n = self
                .file
                .read_bytes(&mut buf[total_read..])
                .map_err(|_| FsError::Invalid)?;
            if n == 0 {
                break;
            }
            total_read += n;
        }

        buf.truncate(total_read);
        crate::debugln!("[FS] ext4: read_to_end read {} bytes", total_read);
        Ok(buf)
    }

    fn size(&mut self) -> u64 {
        self.file.metadata().len()
    }
}

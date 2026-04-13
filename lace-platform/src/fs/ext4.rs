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
        log::debug!("[FS] Loading ext4 filesystem from block device");

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
            log::debug!("[FS] Failed to load ext4 filesystem: {:?}", e);
            FsError::Invalid
        })?;

        log::debug!("[FS] Successfully loaded ext4 filesystem");
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
            log::debug!(
                "[FS] ext4: open_file failed - path is not absolute: {}",
                path
            );
            return Err(FsError::NotFound);
        };

        log::debug!("[FS] ext4: opening file: {}", full_path);
        let file = self.fs.open(full_path).map_err(|e| {
            log::debug!("[FS] ext4: failed to open {}: {:?}", full_path, e);
            match e {
                ext4_view::Ext4Error::NotFound => FsError::NotFound,
                ext4_view::Ext4Error::IsADirectory => FsError::NotDirectory,
                _ => FsError::Invalid,
            }
        })?;

        log::debug!("[FS] ext4: successfully opened file: {}", full_path);
        Ok(Box::new(Ext4File { file }))
    }

    fn file_exists(&mut self, path: &str) -> bool {
        let full_path = if path.starts_with('/') {
            path
        } else {
            log::debug!("[FS] ext4: file_exists - path not absolute: {}", path);
            return false;
        };

        let exists = self.fs.exists(full_path).unwrap_or(false);
        log::debug!("[FS] ext4: file_exists({}) = {}", full_path, exists);
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
        log::debug!("[FS] ext4: read_to_end called");
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
        log::debug!("[FS] ext4: read_to_end read {} bytes", total_read);
        Ok(buf)
    }

    fn size(&mut self) -> u64 {
        self.file.metadata().len()
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use crate::fs::base::{Filesystem, FsError};
    use crate::fs::testutil::MemDisk;

    use alloc::boxed::Box;
    use lace_util::count_blocks_aligned_up;
    use lace_util::tempfile::TempDir;
    use std::process::Command;

    /// Create an ext4 filesystem image populated with the given files.
    ///
    /// Each entry is `(path, contents)` where `path` is an absolute path
    /// (e.g. `/hello.txt` or `/sub/dir/file`). Intermediate directories
    /// are created automatically.
    ///
    /// Requires `mkfs.ext4` on `$PATH`.
    pub(crate) fn make_ext4_image(files: &[(&str, &[u8])]) -> Vec<u8> {
        let dir = TempDir::with_prefix("lace-ext4-test").expect("failed to create temp dir");
        let root = dir.path().join("root");
        std::fs::create_dir_all(&root).unwrap();

        for (path, content) in files {
            let full = root.join(path.trim_start_matches('/'));
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&full, content).unwrap();
        }

        let img_path = dir.path().join("test.ext4");
        let output = Command::new("mkfs.ext4")
            .arg("-d")
            .arg(&root)
            .arg("-b")
            .arg("1024")
            .arg("-O")
            .arg("^has_journal")
            .arg(&img_path)
            .arg("1024")
            .output()
            .expect("mkfs.ext4 not found");
        assert!(
            output.status.success(),
            "mkfs.ext4 failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        std::fs::read(&img_path).unwrap()
        // `dir` is dropped here, cleaning up the temp directory
    }

    /// Wrap raw ext4 image bytes in a `MemDisk` and construct an
    /// `Ext4Filesystem`.
    fn make_filesystem(image: &[u8]) -> Ext4Filesystem {
        let sector_size = 512u32;
        let sector_count = count_blocks_aligned_up!(image.len() as u64, sector_size as u64);
        let mut disk = MemDisk::new(sector_size, sector_count);
        disk.write_at(0, image);
        Ext4Filesystem::new(Box::new(disk)).unwrap()
    }

    // --- Ext4Filesystem::new ---

    #[test]
    fn test_new_valid_image() {
        let image = make_ext4_image(&[("/a.txt", "ok".as_bytes())]);
        let _fs = make_filesystem(&image);
    }

    #[test]
    fn test_new_invalid_data() {
        let garbage = vec![0xFFu8; 1024 * 1024];
        let sector_size = 512u32;
        let sector_count = count_blocks_aligned_up!(garbage.len() as u64, sector_size as u64);
        let mut disk = MemDisk::new(sector_size, sector_count);
        disk.write_at(0, &garbage);

        let result = Ext4Filesystem::new(Box::new(disk));
        assert!(matches!(result, Err(FsError::Invalid)));
    }

    // --- open_file ---

    #[test]
    fn test_open_file_reads_content() {
        let content = "Hello, world!";
        let image = make_ext4_image(&[("/hello.txt", content.as_bytes())]);
        let mut fs = make_filesystem(&image);

        let mut file = fs.open_file("/hello.txt").unwrap();
        let data = file.read_to_end().unwrap();
        assert_eq!(data, content.as_bytes());
    }

    #[test]
    fn test_open_file_relative_path_fails() {
        let image = make_ext4_image(&[("/hello.txt", "data".as_bytes())]);
        let mut fs = make_filesystem(&image);

        let result = fs.open_file("hello.txt");
        assert!(matches!(result, Err(FsError::NotFound)));
    }

    #[test]
    fn test_open_file_not_found() {
        let image = make_ext4_image(&[("/exists.txt", "yes".as_bytes())]);
        let mut fs = make_filesystem(&image);

        let result = fs.open_file("/no_such_file.txt");
        assert!(matches!(result, Err(FsError::NotFound)));
    }

    #[test]
    fn test_open_file_directory_returns_error() {
        let image = make_ext4_image(&[("/subdir/child.txt", "data".as_bytes())]);
        let mut fs = make_filesystem(&image);

        let result = fs.open_file("/subdir");
        assert!(matches!(result, Err(FsError::NotDirectory)));
    }

    // --- file_exists ---

    #[test]
    fn test_file_exists_true() {
        let image = make_ext4_image(&[("/present.txt", "here".as_bytes())]);
        let mut fs = make_filesystem(&image);

        assert!(fs.file_exists("/present.txt"));
    }

    #[test]
    fn test_file_exists_false() {
        let image = make_ext4_image(&[("/present.txt", "here".as_bytes())]);
        let mut fs = make_filesystem(&image);

        assert!(!fs.file_exists("/absent.txt"));
    }

    #[test]
    fn test_file_exists_relative_path() {
        let image = make_ext4_image(&[("/present.txt", "here".as_bytes())]);
        let mut fs = make_filesystem(&image);

        assert!(!fs.file_exists("present.txt"));
    }

    #[test]
    fn test_file_exists_directory() {
        let image = make_ext4_image(&[("/dir/file.txt", "x".as_bytes())]);
        let mut fs = make_filesystem(&image);

        assert!(fs.file_exists("/dir"));
    }

    // --- read_dir ---

    #[test]
    fn test_read_dir_root() {
        let image = make_ext4_image(&[
            ("/alpha.txt", "a".as_bytes()),
            ("/beta.txt", "b".as_bytes()),
            ("/subdir/nested.txt", "n".as_bytes()),
        ]);
        let mut fs = make_filesystem(&image);

        let mut entries = fs.read_dir("/").unwrap();
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(
            names.contains(&"alpha.txt"),
            "expected alpha.txt in {:?}",
            names
        );
        assert!(
            names.contains(&"beta.txt"),
            "expected beta.txt in {:?}",
            names
        );
        assert!(names.contains(&"subdir"), "expected subdir in {:?}", names);

        // Verify . and .. are filtered out
        assert!(!names.contains(&"."), ". should be filtered");
        assert!(!names.contains(&".."), ".. should be filtered");

        // Verify is_dir flags
        let subdir_entry = entries.iter().find(|e| e.name == "subdir").unwrap();
        assert!(subdir_entry.is_dir, "subdir should be marked as directory");

        let alpha_entry = entries.iter().find(|e| e.name == "alpha.txt").unwrap();
        assert!(!alpha_entry.is_dir, "alpha.txt should not be a directory");
    }

    #[test]
    fn test_read_dir_subdirectory() {
        let image = make_ext4_image(&[
            ("/sub/one.txt", "1".as_bytes()),
            ("/sub/two.txt", "2".as_bytes()),
        ]);
        let mut fs = make_filesystem(&image);

        let mut entries = fs.read_dir("/sub").unwrap();
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["one.txt", "two.txt"]);
    }

    #[test]
    fn test_read_dir_not_found() {
        let image = make_ext4_image(&[("/a.txt", "a".as_bytes())]);
        let mut fs = make_filesystem(&image);

        let result = fs.read_dir("/nonexistent");
        assert!(matches!(result, Err(FsError::NotFound)));
    }

    #[test]
    fn test_read_dir_relative_path_fails() {
        let image = make_ext4_image(&[("/dir/f.txt", "x".as_bytes())]);
        let mut fs = make_filesystem(&image);

        let result = fs.read_dir("dir");
        assert!(matches!(result, Err(FsError::NotFound)));
    }

    // --- File trait: read, read_to_end, size ---

    #[test]
    fn test_file_read_partial() {
        let image = make_ext4_image(&[("/data.bin", "ABCDEFGHIJ".as_bytes())]);
        let mut fs = make_filesystem(&image);

        let mut file = fs.open_file("/data.bin").unwrap();
        let mut buf = [0u8; 4];
        let n = file.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], "ABCD".as_bytes());
    }

    #[test]
    fn test_file_read_to_end() {
        let content = "The quick brown fox jumps over the lazy dog";
        let image = make_ext4_image(&[("/fox.txt", content.as_bytes())]);
        let mut fs = make_filesystem(&image);

        let mut file = fs.open_file("/fox.txt").unwrap();
        let data = file.read_to_end().unwrap();
        assert_eq!(data, content.as_bytes());
    }

    #[test]
    fn test_file_read_to_end_empty() {
        let image = make_ext4_image(&[("/empty.txt", "".as_bytes())]);
        let mut fs = make_filesystem(&image);

        let mut file = fs.open_file("/empty.txt").unwrap();
        let data = file.read_to_end().unwrap();
        assert!(data.is_empty());
    }

    #[test]
    fn test_file_size() {
        let content = vec![0x42u8; 1234];
        let image = make_ext4_image(&[("/sized.bin", &content)]);
        let mut fs = make_filesystem(&image);

        let mut file = fs.open_file("/sized.bin").unwrap();
        assert_eq!(file.size(), 1234);
    }

    #[test]
    fn test_file_size_empty() {
        let image = make_ext4_image(&[("/empty.txt", "".as_bytes())]);
        let mut fs = make_filesystem(&image);

        let mut file = fs.open_file("/empty.txt").unwrap();
        assert_eq!(file.size(), 0);
    }
}

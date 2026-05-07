// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! CBFS image reader.

use super::r#priv::{CbfsEntryWalker, parse_header};
use super::*;
use crate::UsizeIsAtLeastU32;

/// A parsed CBFS image backed by a memory-mapped ROM region.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CbfsReader<'a> {
    rom: &'a [u8],
    align: u32,
    offset: u32,
}

/// A file found in a CBFS image.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CbfsFile<'a> {
    pub name: &'a [u8],
    pub data: &'a [u8],
    pub r#type: u32,
    /// Absolute offset of the file data within the ROM.
    pub offset: u32,
}

/// Iterator over CBFS file entries.
pub struct CbfsFiles<'a> {
    rom: &'a [u8],
    walker: CbfsEntryWalker,
}

impl<'a> Iterator for CbfsFiles<'a> {
    type Item = Result<CbfsFile<'a>, CbfsError>;

    fn next(&mut self) -> Option<Result<CbfsFile<'a>, CbfsError>> {
        let entry = match self.walker.next(self.rom)? {
            Ok(e) => e,
            Err(e) => return Some(Err(e)),
        };
        Some(Ok(CbfsFile {
            name: &self.rom
                [entry.name_start.as_usize()..(entry.name_start + entry.name_len).as_usize()],
            data: &self.rom
                [entry.data_start.as_usize()..(entry.data_start + entry.data_len).as_usize()],
            r#type: entry.entry_type,
            offset: entry.data_start,
        }))
    }
}

impl<'a> CbfsReader<'a> {
    /// Open an existing CBFS image.
    ///
    /// `rom_base` is the absolute address where the ROM is mapped (e.g.
    /// `0xFFC00000` for a 4MB ROM).
    pub fn open(rom: &'a [u8], rom_base: u32) -> Result<Self, CbfsError> {
        let (_, align, offset) = parse_header(rom, rom_base)?;
        Ok(Self { rom, align, offset })
    }

    /// Return an iterator over all file entries in the image.
    pub fn files(&self) -> CbfsFiles<'a> {
        CbfsFiles {
            rom: self.rom,
            walker: CbfsEntryWalker::new(self.align, self.offset),
        }
    }

    /// Find a file by name.
    ///
    /// Returns `Ok(None)` if no matching file was found, or
    /// `Err(CbfsError::CorruptEntry)` if a corrupt entry was encountered
    /// before the file could be located.
    pub fn find_file(&self, name: &[u8]) -> Result<Option<CbfsFile<'a>>, CbfsError> {
        for result in self.files() {
            let f = result?;
            if f.r#type != CBFS_TYPE_NULL && f.r#type != CBFS_TYPE_DELETED && f.name == name {
                return Ok(Some(f));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::cbfs::CbfsWriter;

    const ROM_SIZE: u32 = 4096;
    const ROM_BASE: u32 = 0xFFFF_F000;

    fn build_test_rom(files: &[(&[u8], u32, &[u8])]) -> Vec<u8> {
        let mut rom = vec![0xFFu8; ROM_SIZE.as_usize()];
        {
            let mut writer =
                CbfsWriter::create(&mut rom, ROM_BASE, CBFS_DEFAULT_ALIGNMENT).unwrap();
            for &(name, r#type, data) in files {
                writer.add_file(None, name, r#type, data).unwrap();
            }
        }
        rom
    }

    #[test]
    fn test_open_and_iterate() {
        let rom = build_test_rom(&[]);
        let cbfs = CbfsReader::open(&rom, ROM_BASE).unwrap();
        assert_eq!(cbfs.files().count(), 2); // master header + NULL

        let rom = build_test_rom(&[
            (b"file/one", CBFS_TYPE_RAW, b"first"),
            (b"file/two", CBFS_TYPE_RAW, b"second"),
        ]);
        let cbfs = CbfsReader::open(&rom, ROM_BASE).unwrap();
        let names: Vec<_> = cbfs.files().map(|f| f.unwrap().name).collect();
        assert_eq!(
            names,
            &[
                b"cbfs_master_header".as_ref(),
                b"file/one",
                b"file/two",
                b"",
            ]
        );
    }

    #[test]
    fn test_find_file() {
        let rom = build_test_rom(&[
            (b"null", CBFS_TYPE_NULL, b"x"),
            (b"deleted", CBFS_TYPE_DELETED, b"x"),
            (b"test/hello", CBFS_TYPE_RAW, b"Hello CBFS!"),
        ]);
        let cbfs = CbfsReader::open(&rom, ROM_BASE).unwrap();

        let file = cbfs.find_file(b"test/hello").unwrap().unwrap();
        assert_eq!(file.name, b"test/hello");
        assert_eq!(file.data, b"Hello CBFS!");
        assert_eq!(file.r#type, CBFS_TYPE_RAW);

        // Skips NULL/DELETED types and missing names
        assert!(cbfs.find_file(b"null").unwrap().is_none());
        assert!(cbfs.find_file(b"deleted").unwrap().is_none());
        assert!(cbfs.find_file(b"nonexistent").unwrap().is_none());
    }
}

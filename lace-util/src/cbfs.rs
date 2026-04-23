// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! CBFS (coreboot filesystem) structures and parser
//!
//! Provides both the on-flash data structures (for reading and writing CBFS
//! images) and a read-only parser for memory-mapped flash.

use zerocopy::byteorder::{BE, U32};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

// Public constants for CBFS format

/// CBFS header magic: 'ORBC' in big-endian.
pub const CBFS_HEADER_MAGIC: u32 = 0x4F524243;
/// CBFS header version 2: '1112' in big-endian.
pub const CBFS_HEADER_VERSION2: u32 = 0x31313132;
/// CBFS file magic: "LARCHIVE".
pub const CBFS_FILE_MAGIC: [u8; 8] = *b"LARCHIVE";
/// CBFS alignment (fixed to 64 bytes).
pub const CBFS_ALIGNMENT: usize = 64;
/// CBFS architecture: x86.
pub const CBFS_ARCHITECTURE_X86: u32 = 0x00000001;

// File types
pub const CBFS_TYPE_DELETED: u32 = 0x00000000;
pub const CBFS_TYPE_NULL: u32 = 0xFFFFFFFF;
pub const CBFS_TYPE_BOOTBLOCK: u32 = 0x01;
pub const CBFS_TYPE_RAW: u32 = 0x50;

/// CBFS master header (on-flash format, all big-endian).
#[repr(C)]
#[derive(Clone, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct CbfsHeader {
    pub magic: U32<BE>,
    pub version: U32<BE>,
    pub romsize: U32<BE>,
    pub bootblocksize: U32<BE>,
    pub align: U32<BE>,
    pub offset: U32<BE>,
    pub architecture: U32<BE>,
    pub _pad: U32<BE>,
}

/// CBFS file entry header (on-flash format, all big-endian).
#[repr(C)]
#[derive(Clone, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct CbfsFileHeader {
    pub magic: [u8; 8],
    pub len: U32<BE>,
    pub type_: U32<BE>,
    pub attributes_offset: U32<BE>,
    pub offset: U32<BE>,
}

/// Size of the file header in bytes.
pub const CBFS_FILE_HEADER_SIZE: usize = size_of::<CbfsFileHeader>();

/// Calculate total header size (fixed header + filename + null + padding to alignment).
pub fn cbfs_file_header_total_size(name: &str) -> usize {
    cbfs_align_up(CBFS_FILE_HEADER_SIZE + name.len() + 1)
}

/// Align a value up to CBFS alignment.
pub fn cbfs_align_up(value: usize) -> usize {
    (value + CBFS_ALIGNMENT - 1) & !(CBFS_ALIGNMENT - 1)
}

// --- Read-only parser for memory-mapped flash ---

/// A file found in a CBFS image.
pub struct CbfsFile<'a> {
    pub name: &'a str,
    pub data: &'a [u8],
    pub type_: u32,
}

/// A parsed CBFS image backed by a memory-mapped ROM region.
pub struct Cbfs<'a> {
    rom: &'a [u8],
    entries_offset: usize,
    bootblock_size: usize,
}

impl<'a> Cbfs<'a> {
    /// Parse a CBFS image from a memory-mapped ROM slice.
    ///
    /// `rom_base` is the absolute address where the ROM is mapped (e.g.
    /// `0xFFC00000` for a 4MB ROM). This is needed to resolve the header
    /// pointer at the last 4 bytes of the ROM, which stores an absolute address.
    pub fn parse(rom: &'a [u8], rom_base: u32) -> Option<Self> {
        if rom.len() < 4 {
            return None;
        }

        // Read header pointer from last 4 bytes of ROM (little-endian absolute address)
        let ptr_bytes: [u8; 4] = rom[rom.len() - 4..].try_into().ok()?;
        let header_addr = u32::from_le_bytes(ptr_bytes);
        let header_offset = header_addr.checked_sub(rom_base)? as usize;

        if header_offset + size_of::<CbfsHeader>() > rom.len() {
            return None;
        }

        let (header, _) = CbfsHeader::read_from_prefix(&rom[header_offset..]).ok()?;
        if header.magic.get() != CBFS_HEADER_MAGIC {
            return None;
        }

        let bootblock_size = header.bootblocksize.get() as usize;
        if bootblock_size > rom.len() {
            return None;
        }

        Some(Self {
            rom,
            entries_offset: header.offset.get() as usize,
            bootblock_size,
        })
    }

    /// Iterate over all files, calling `f` for each.
    ///
    /// The callback returns `true` to continue, `false` to stop.
    pub fn for_each_file(&self, mut f: impl FnMut(&CbfsFile<'a>) -> bool) {
        let end = self.rom.len() - self.bootblock_size;
        let mut cursor = self.entries_offset;

        while cursor + CBFS_FILE_HEADER_SIZE <= end {
            let Some((hdr, _)) = CbfsFileHeader::read_from_prefix(&self.rom[cursor..]).ok() else {
                break;
            };

            if hdr.magic != CBFS_FILE_MAGIC {
                break;
            }

            let type_ = hdr.type_.get();
            if type_ == CBFS_TYPE_DELETED {
                break;
            }

            let data_offset = hdr.offset.get() as usize;
            let data_len = hdr.len.get() as usize;
            let data_start = cursor + data_offset;

            if data_start + data_len > end {
                break;
            }

            // Advance cursor to next aligned entry
            let entry_size = cbfs_align_up(data_offset + data_len);
            let next_cursor = cursor + entry_size;

            // Skip null/empty entries
            if type_ != CBFS_TYPE_NULL {
                // Extract null-terminated filename
                let name_start = cursor + CBFS_FILE_HEADER_SIZE;
                let name_bytes = &self.rom[name_start..cursor + data_offset];
                let name_len = name_bytes
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(name_bytes.len());
                let name = core::str::from_utf8(&name_bytes[..name_len]).unwrap_or("");

                let file = CbfsFile {
                    name,
                    data: &self.rom[data_start..data_start + data_len],
                    type_,
                };
                if !f(&file) {
                    return;
                }
            }

            cursor = next_cursor;
        }
    }

    /// Get the bootblock size.
    pub fn bootblock_size(&self) -> usize {
        self.bootblock_size
    }

    /// Find a file by name.
    pub fn find_file(&self, name: &str) -> Option<CbfsFile<'a>> {
        let mut result = None;
        self.for_each_file(|file| {
            if file.name == name {
                result = Some(CbfsFile {
                    name: file.name,
                    data: file.data,
                    type_: file.type_,
                });
                false
            } else {
                true
            }
        });
        result
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use zerocopy::IntoBytes;
    use zerocopy::byteorder::U32;

    const ROM_SIZE: usize = 4096;
    const BB_SIZE: usize = 256;
    const ROM_BASE: u32 = 0xFFFF_F000; // 4GB - ROM_SIZE

    /// Build a minimal CBFS ROM image with the given files.
    fn build_test_rom(files: &[(&str, u32, &[u8])]) -> Vec<u8> {
        let mut rom = vec![0xFFu8; ROM_SIZE];
        let mut cursor = 0usize;

        // Write master header as first file
        let header = CbfsHeader {
            magic: U32::new(CBFS_HEADER_MAGIC),
            version: U32::new(CBFS_HEADER_VERSION2),
            romsize: U32::new(ROM_SIZE as u32),
            bootblocksize: U32::new(BB_SIZE as u32),
            align: U32::new(CBFS_ALIGNMENT as u32),
            offset: U32::new(0),
            architecture: U32::new(CBFS_ARCHITECTURE_X86),
            _pad: U32::new(0),
        };
        let hdr_name = "cbfs_master_header";
        let hdr_total = cbfs_file_header_total_size(hdr_name);
        let hdr_data = header.as_bytes();
        let entry_size = cbfs_align_up(hdr_total + hdr_data.len());
        let file_hdr = CbfsFileHeader {
            magic: CBFS_FILE_MAGIC,
            len: U32::new(hdr_data.len() as u32),
            type_: U32::new(CBFS_TYPE_RAW),
            attributes_offset: U32::new(0),
            offset: U32::new(hdr_total as u32),
        };
        // Zero the entry area for null termination
        rom[cursor..cursor + entry_size].fill(0);
        rom[cursor..cursor + CBFS_FILE_HEADER_SIZE].copy_from_slice(file_hdr.as_bytes());
        rom[cursor + CBFS_FILE_HEADER_SIZE..cursor + CBFS_FILE_HEADER_SIZE + hdr_name.len()]
            .copy_from_slice(hdr_name.as_bytes());
        rom[cursor + hdr_total..cursor + hdr_total + hdr_data.len()].copy_from_slice(hdr_data);
        let header_data_offset = cursor + hdr_total;
        cursor += entry_size;

        // Write user files
        for &(name, type_, data) in files {
            let total = cbfs_file_header_total_size(name);
            let entry_len = cbfs_align_up(total + data.len());
            let fh = CbfsFileHeader {
                magic: CBFS_FILE_MAGIC,
                len: U32::new(data.len() as u32),
                type_: U32::new(type_),
                attributes_offset: U32::new(0),
                offset: U32::new(total as u32),
            };
            rom[cursor..cursor + entry_len].fill(0);
            rom[cursor..cursor + CBFS_FILE_HEADER_SIZE].copy_from_slice(fh.as_bytes());
            rom[cursor + CBFS_FILE_HEADER_SIZE..cursor + CBFS_FILE_HEADER_SIZE + name.len()]
                .copy_from_slice(name.as_bytes());
            rom[cursor + total..cursor + total + data.len()].copy_from_slice(data);
            cursor += cbfs_align_up(total + data.len());
        }

        // Write header pointer at last 4 bytes
        let header_addr = ROM_BASE + header_data_offset as u32;
        rom[ROM_SIZE - 4..ROM_SIZE].copy_from_slice(&header_addr.to_le_bytes());

        rom
    }

    #[test]
    fn test_parse_empty_cbfs() {
        let rom = build_test_rom(&[]);
        let cbfs = Cbfs::parse(&rom, ROM_BASE).unwrap();
        assert_eq!(cbfs.bootblock_size(), BB_SIZE);

        let mut count = 0;
        cbfs.for_each_file(|_| {
            count += 1;
            true
        });
        // Only the master header file
        assert_eq!(count, 1);
    }

    #[test]
    fn test_find_file() {
        let rom = build_test_rom(&[("test/hello", CBFS_TYPE_RAW, b"Hello CBFS!")]);
        let cbfs = Cbfs::parse(&rom, ROM_BASE).unwrap();

        let file = cbfs.find_file("test/hello").unwrap();
        assert_eq!(file.name, "test/hello");
        assert_eq!(file.data, b"Hello CBFS!");
        assert_eq!(file.type_, CBFS_TYPE_RAW);
    }

    #[test]
    fn test_find_file_not_found() {
        let rom = build_test_rom(&[("test/hello", CBFS_TYPE_RAW, b"data")]);
        let cbfs = Cbfs::parse(&rom, ROM_BASE).unwrap();
        assert!(cbfs.find_file("nonexistent").is_none());
    }

    #[test]
    fn test_multiple_files() {
        let rom = build_test_rom(&[
            ("file/one", CBFS_TYPE_RAW, b"first"),
            ("file/two", CBFS_TYPE_RAW, b"second"),
        ]);
        let cbfs = Cbfs::parse(&rom, ROM_BASE).unwrap();

        let mut names = alloc::vec::Vec::new();
        cbfs.for_each_file(|f| {
            names.push(alloc::string::String::from(f.name));
            true
        });
        assert_eq!(names, &["cbfs_master_header", "file/one", "file/two"]);
    }

    #[test]
    fn test_bad_magic() {
        let mut rom = vec![0u8; ROM_SIZE];
        // Header pointer at end points to offset 0, but no valid header there
        rom[ROM_SIZE - 4..ROM_SIZE].copy_from_slice(&ROM_BASE.to_le_bytes());
        assert!(Cbfs::parse(&rom, ROM_BASE).is_none());
    }

    #[test]
    fn test_struct_sizes() {
        assert_eq!(size_of::<CbfsHeader>(), 32);
        assert_eq!(size_of::<CbfsFileHeader>(), 24);
        assert_eq!(CBFS_FILE_HEADER_SIZE, 24);
    }
}

// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! CBFS image writer.
//!
//! Free space in a CBFS image is represented as `CBFS_TYPE_NULL` entries.
//! Adding a file finds a suitable NULL slot and splits it; deleting a file
//! marks it as `CBFS_TYPE_DELETED` and merges adjacent empty entries, matching
//! the semantics of coreboot's cbfstool.

use zerocopy::byteorder::{LE, U32};
use zerocopy::{FromBytes, IntoBytes};

use super::r#priv::{CbfsEntryWalker, parse_header};
use super::*;
use crate::{UsizeIsAtLeastU32, align_up_checked, const_u32};

/// Minimum metadata size: header + one NUL byte for an empty name.
const MIN_ENTRY_METADATA: u32 = const_u32(size_of::<CbfsFileHeader>() + 1);

/// A CBFS image writer operating on a caller-provided ROM buffer.
///
/// Can initialize a fresh image or open an existing one for modification.
/// Free space is tracked as `CBFS_TYPE_NULL` entries; no append cursor is
/// needed. All operations use checked arithmetic and bounds validation.
pub struct CbfsWriter<'a> {
    rom: &'a mut [u8],
    align: u32,
    offset: u32,
}

impl<'a> CbfsWriter<'a> {
    /// Initialize a fresh CBFS image in `rom`.
    ///
    /// Fills `rom` with `0xFF`, writes the master header as the first file
    /// entry, and creates a single large `CBFS_TYPE_NULL` entry covering all
    /// remaining space. The header pointer is written at the last 4 bytes.
    pub fn create(rom: &'a mut [u8], rom_base: u32, align: u32) -> Result<Self, CbfsError> {
        let rom_size: u32 = rom
            .len()
            .try_into()
            .ok()
            .filter(|s: &u32| s.is_power_of_two())
            .ok_or(CbfsError::InvalidRomSize)?;

        if !rom_base.is_multiple_of(rom_size) {
            return Err(CbfsError::InvalidRomBase);
        }
        if !align.is_power_of_two() || align > rom_size {
            return Err(CbfsError::InvalidAlignment);
        }

        rom.fill(0xFF);

        let mut writer = Self {
            rom,
            align,
            offset: 0,
        };

        // Write master header as the first file entry
        let header = CbfsHeader {
            magic: CBFS_HEADER_MAGIC.into(),
            version: CBFS_HEADER_VERSION2.into(),
            romsize: rom_size.into(),
            bootblocksize: 4.into(),
            align: align.into(),
            offset: 0.into(),
            architecture: CBFS_ARCHITECTURE_X86.into(),
            _pad: 0.into(),
        };
        let name = b"cbfs_master_header";
        let data = header.as_bytes();
        let data_len = const_u32(size_of::<CbfsHeader>());
        let data_offset = const_u32(size_of::<CbfsFileHeader>() + name.len() + 1);
        writer.write_entry(0, name, CBFS_TYPE_RAW, data, data_len, data_offset);

        // Write master header pointer at last 4 bytes
        let ptr: U32<LE> = rom_base
            .checked_add(data_offset)
            .ok_or(CbfsError::InvalidRomBase)?
            .into();
        ptr.write_to_suffix(writer.rom)
            .map_err(|_| CbfsError::InvalidRomSize)?;

        // Create a NULL entry spanning all remaining space before the
        // 4-byte header pointer at the end of the ROM.
        let null_start =
            align_up_checked!(data_offset + data_len, align).ok_or(CbfsError::InvalidRomSize)?;
        let entries_end = rom_size.checked_sub(4).ok_or(CbfsError::InvalidRomSize)?;
        if entries_end > null_start + MIN_ENTRY_METADATA {
            let null_len = entries_end - null_start - MIN_ENTRY_METADATA;
            writer.write_null_entry(null_start, null_len);
        }

        Ok(writer)
    }

    /// Open an existing CBFS image for modification.
    pub fn open(rom: &'a mut [u8], rom_base: u32) -> Result<Self, CbfsError> {
        let (_, align, offset) = parse_header(rom, rom_base)?;
        Ok(Self { rom, align, offset })
    }

    /// Append a file to the CBFS.
    ///
    /// Walks entries looking for a `CBFS_TYPE_NULL` slot large enough to
    /// hold the new file. If `content_offset` is specified, the file data
    /// must begin at that ROM offset. Merges adjacent empty entries before
    /// searching (matching cbfstool semantics).
    pub fn add_file(
        &mut self,
        content_offset: Option<u32>,
        name: &[u8],
        r#type: u32,
        data: &[u8],
    ) -> Result<(), CbfsError> {
        if name.contains(&0) {
            return Err(CbfsError::NulInName);
        }

        let data_len: u32 = data.len().try_into().map_err(|_| CbfsError::TooLarge)?;
        let name_len: u32 = name.len().try_into().map_err(|_| CbfsError::TooLarge)?;

        self.merge_empty_entries()?;

        let hdr_size = const_u32(size_of::<CbfsFileHeader>());
        let header_size = align_up_checked!(
            hdr_size
                .checked_add(name_len)
                .and_then(|n| n.checked_add(1))
                .ok_or(CbfsError::TooLarge)?,
            self.align
        )
        .ok_or(CbfsError::TooLarge)?;
        let need_size = header_size
            .checked_add(data_len)
            .ok_or(CbfsError::TooLarge)?;

        // Walk entries looking for a suitable NULL slot
        let mut walker = CbfsEntryWalker::new(self.align, self.offset);
        while let Some(result) = walker.next(self.rom) {
            let entry = result?;
            let slot_size = entry.entry_end - entry.entry_start;

            if entry.entry_type != CBFS_TYPE_NULL || slot_size < need_size {
                continue;
            }

            // Check content_offset constraints
            if let Some(co) = content_offset {
                let co_end = co.checked_add(data_len).ok_or(CbfsError::TooLarge)?;
                if co < entry.entry_start + header_size || co_end > entry.entry_end {
                    continue;
                }
            }

            let co = content_offset.unwrap_or(entry.entry_start + header_size);
            let co_end = co.checked_add(data_len).ok_or(CbfsError::TooLarge)?;
            let new_entry_start = (co - header_size) & !(self.align - 1);

            // Leading gap: create a NULL entry if big enough
            let min_aligned =
                align_up_checked!(MIN_ENTRY_METADATA, self.align).ok_or(CbfsError::TooLarge)?;
            if new_entry_start > entry.entry_start {
                let leading = new_entry_start - entry.entry_start;
                if leading >= min_aligned {
                    let null_len = leading - MIN_ENTRY_METADATA;
                    self.write_null_entry(entry.entry_start, null_len);
                }
            }

            // Write the file entry
            let hdr_start = new_entry_start.max(entry.entry_start);
            let data_offset = co - hdr_start;
            self.write_entry(hdr_start, name, r#type, data, data_len, data_offset);

            // Trailing gap: create a NULL entry if big enough
            let file_end = align_up_checked!(co_end, self.align).ok_or(CbfsError::TooLarge)?;
            if entry.entry_end > file_end {
                let trailing = entry.entry_end - file_end;
                if trailing >= min_aligned {
                    let null_len = trailing - MIN_ENTRY_METADATA;
                    self.write_null_entry(file_end, null_len);
                }
            }

            return Ok(());
        }

        Err(CbfsError::NoSpace)
    }

    /// Delete the file with the given name.
    ///
    /// Marks the entry as `CBFS_TYPE_DELETED` and merges adjacent empty
    /// entries. Returns `Ok(true)` if found and deleted, `Ok(false)` if
    /// not found.
    ///
    /// Note: if a corrupt entry is encountered before the file is found,
    /// `Err(CbfsError::CorruptEntry)` is returned and the image may have
    /// been partially modified by a preceding merge pass.
    pub fn delete_file(&mut self, name: &[u8]) -> Result<bool, CbfsError> {
        let mut walker = CbfsEntryWalker::new(self.align, self.offset);
        while let Some(result) = walker.next(self.rom) {
            let entry = result?;
            if &self.rom
                [entry.name_start.as_usize()..(entry.name_start + entry.name_len).as_usize()]
                == name
            {
                let (hdr, _) =
                    CbfsFileHeader::mut_from_prefix(&mut self.rom[entry.entry_start.as_usize()..])
                        .expect("unreachable");
                hdr.r#type = CBFS_TYPE_DELETED.into();
                self.merge_empty_entries()?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Merge consecutive NULL/DELETED entries into single NULL entries.
    ///
    /// Returns `Err(CbfsError::CorruptEntry)` if a corrupt entry is
    /// encountered; entries before the corrupt one may already have been
    /// merged.
    fn merge_empty_entries(&mut self) -> Result<(), CbfsError> {
        let mut walker = CbfsEntryWalker::new(self.align, self.offset);
        while let Some(result) = walker.next(self.rom) {
            let entry = result?;
            if entry.entry_type != CBFS_TYPE_NULL && entry.entry_type != CBFS_TYPE_DELETED {
                continue;
            }

            // Walk forward, absorbing consecutive empty entries
            let mut merge_end = entry.entry_end;
            let mut inner = CbfsEntryWalker::new(
                self.align,
                align_up_checked!(merge_end, self.align).ok_or(CbfsError::CorruptEntry)?,
            );
            while let Some(result) = inner.next(self.rom) {
                let next = result?;
                if next.entry_type != CBFS_TYPE_NULL && next.entry_type != CBFS_TYPE_DELETED {
                    break;
                }
                merge_end = next.entry_end;
            }

            if merge_end > entry.entry_end {
                let null_len = merge_end - entry.entry_start - MIN_ENTRY_METADATA;
                self.write_null_entry(entry.entry_start, null_len);
            }
        }
        Ok(())
    }

    /// Write a file entry (header + NUL-terminated name + data) at
    /// `entry_start`. `data_offset` is the distance from `entry_start`
    /// to the data. `data_len` is the content length stored in the
    /// header (must equal `data.len()` or be larger for NULL padding).
    ///
    /// # Panics
    ///
    /// Panics if offsets are out of bounds. Callers must validate
    /// placement before calling.
    fn write_entry(
        &mut self,
        entry_start: u32,
        name: &[u8],
        r#type: u32,
        data: &[u8],
        data_len: u32,
        data_offset: u32,
    ) {
        let hdr = CbfsFileHeader {
            magic: CBFS_FILE_MAGIC,
            len: data_len.into(),
            r#type: r#type.into(),
            attributes_offset: 0.into(),
            offset: data_offset.into(),
        };

        let start = entry_start.as_usize();
        let hdr_end = start + size_of::<CbfsFileHeader>();
        let name_end = hdr_end + name.len();
        let data_start = start + data_offset.as_usize();
        let data_end = data_start + data.len();

        self.rom[start..hdr_end].copy_from_slice(hdr.as_bytes());
        self.rom[hdr_end..name_end].copy_from_slice(name);
        self.rom[name_end] = 0;
        self.rom[data_start..data_end].copy_from_slice(data);
    }

    /// Write a NULL (free space) entry at `start` with the given content
    /// length.
    ///
    /// # Panics
    ///
    /// Panics if offsets are out of bounds. Callers must validate
    /// placement before calling.
    fn write_null_entry(&mut self, start: u32, len: u32) {
        self.write_entry(start, b"", CBFS_TYPE_NULL, &[], len, MIN_ENTRY_METADATA);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::cbfs::CbfsReader;

    const ROM_SIZE: u32 = 4096;
    const ROM_BASE: u32 = 0xFFFF_F000;

    fn new_image() -> Vec<u8> {
        let mut rom = vec![0u8; ROM_SIZE.as_usize()];
        CbfsWriter::create(&mut rom, ROM_BASE, CBFS_DEFAULT_ALIGNMENT).unwrap();
        rom
    }

    #[test]
    fn test_create_roundtrip() {
        let mut rom = new_image();
        {
            let mut w = CbfsWriter::open(&mut rom, ROM_BASE).unwrap();
            w.add_file(None, b"hello", CBFS_TYPE_RAW, b"world").unwrap();
            // Place a file at a specific content offset (leading gap path)
            w.add_file(Some(ROM_SIZE - 128), b"placed", CBFS_TYPE_RAW, b"here")
                .unwrap();
        }

        let reader = CbfsReader::open(&rom, ROM_BASE).unwrap();
        assert_eq!(reader.find_file(b"hello").unwrap().unwrap().data, b"world");
        assert_eq!(reader.find_file(b"placed").unwrap().unwrap().data, b"here");
    }

    #[test]
    fn test_create_rejects_bad_alignment() {
        let mut rom = vec![0u8; ROM_SIZE.as_usize()];
        assert!(matches!(
            CbfsWriter::create(&mut rom, ROM_BASE, 3),
            Err(CbfsError::InvalidAlignment)
        ));
    }

    #[test]
    fn test_create_rejects_alignment_larger_than_rom() {
        let mut rom = vec![0u8; ROM_SIZE.as_usize()];
        assert!(matches!(
            CbfsWriter::create(&mut rom, ROM_BASE, ROM_SIZE * 2),
            Err(CbfsError::InvalidAlignment)
        ));
    }

    #[test]
    fn test_open_and_append() {
        let mut rom = new_image();
        CbfsWriter::open(&mut rom, ROM_BASE)
            .unwrap()
            .add_file(None, b"first", CBFS_TYPE_RAW, b"1")
            .unwrap();
        CbfsWriter::open(&mut rom, ROM_BASE)
            .unwrap()
            .add_file(None, b"second", CBFS_TYPE_RAW, b"2")
            .unwrap();

        let reader = CbfsReader::open(&rom, ROM_BASE).unwrap();
        assert_eq!(reader.find_file(b"first").unwrap().unwrap().data, b"1");
        assert_eq!(reader.find_file(b"second").unwrap().unwrap().data, b"2");
    }

    #[test]
    fn test_delete_file() {
        let mut rom = new_image();
        {
            let mut w = CbfsWriter::open(&mut rom, ROM_BASE).unwrap();
            w.add_file(None, b"keep", CBFS_TYPE_RAW, b"yes").unwrap();
            w.add_file(None, b"remove", CBFS_TYPE_RAW, b"no").unwrap();
        }
        {
            let mut w = CbfsWriter::open(&mut rom, ROM_BASE).unwrap();
            assert_eq!(w.delete_file(b"remove"), Ok(true));
            assert_eq!(w.delete_file(b"nonexistent"), Ok(false));
        }

        let reader = CbfsReader::open(&rom, ROM_BASE).unwrap();
        assert_eq!(reader.find_file(b"keep").unwrap().unwrap().data, b"yes");
        assert!(reader.find_file(b"remove").unwrap().is_none());
    }

    #[test]
    fn test_delete_and_reuse_space() {
        let mut rom = new_image();
        {
            let mut w = CbfsWriter::open(&mut rom, ROM_BASE).unwrap();
            w.add_file(None, b"a", CBFS_TYPE_RAW, b"aaaa").unwrap();
            w.add_file(None, b"b", CBFS_TYPE_RAW, b"bbbb").unwrap();
            w.add_file(None, b"c", CBFS_TYPE_RAW, b"cccc").unwrap();
        }
        {
            let mut w = CbfsWriter::open(&mut rom, ROM_BASE).unwrap();
            assert_eq!(w.delete_file(b"b"), Ok(true));
            w.add_file(None, b"d", CBFS_TYPE_RAW, b"dd").unwrap();
        }

        let reader = CbfsReader::open(&rom, ROM_BASE).unwrap();
        assert_eq!(reader.find_file(b"a").unwrap().unwrap().data, b"aaaa");
        assert!(reader.find_file(b"b").unwrap().is_none());
        assert_eq!(reader.find_file(b"c").unwrap().unwrap().data, b"cccc");
        assert_eq!(reader.find_file(b"d").unwrap().unwrap().data, b"dd");
    }

    #[test]
    fn test_delete_adjacent_merges() {
        let mut rom = new_image();
        let large_data = vec![0x42u8; 256];
        {
            let mut w = CbfsWriter::open(&mut rom, ROM_BASE).unwrap();
            w.add_file(None, b"a", CBFS_TYPE_RAW, b"aa").unwrap();
            w.add_file(None, b"b", CBFS_TYPE_RAW, b"bb").unwrap();
            w.add_file(None, b"c", CBFS_TYPE_RAW, b"cc").unwrap();
        }
        {
            let mut w = CbfsWriter::open(&mut rom, ROM_BASE).unwrap();
            assert_eq!(w.delete_file(b"a"), Ok(true));
            assert_eq!(w.delete_file(b"b"), Ok(true));
            w.add_file(None, b"big", CBFS_TYPE_RAW, &large_data)
                .unwrap();
        }

        let reader = CbfsReader::open(&rom, ROM_BASE).unwrap();
        assert_eq!(
            reader.find_file(b"big").unwrap().unwrap().data,
            &large_data[..]
        );
        assert_eq!(reader.find_file(b"c").unwrap().unwrap().data, b"cc");
    }

    #[test]
    fn test_add_file_rejects_invalid() {
        let mut rom = new_image();
        let mut w = CbfsWriter::open(&mut rom, ROM_BASE).unwrap();
        // NUL in name
        assert!(matches!(
            w.add_file(None, b"bad\0name", CBFS_TYPE_RAW, b"x"),
            Err(CbfsError::NulInName)
        ));
        // Data too large for ROM
        assert!(matches!(
            w.add_file(
                None,
                b"huge",
                CBFS_TYPE_RAW,
                &vec![0u8; ROM_SIZE.as_usize()]
            ),
            Err(CbfsError::NoSpace)
        ));
        // content_offset too early (no room for header)
        assert!(matches!(
            w.add_file(Some(0), b"bad", CBFS_TYPE_RAW, b"x"),
            Err(CbfsError::NoSpace)
        ));
    }
}

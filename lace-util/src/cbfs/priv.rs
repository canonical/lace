// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! Internal CBFS helpers shared between reader and writer.

use zerocopy::FromBytes;
use zerocopy::byteorder::{LE, U32};

use super::*;
use crate::{UsizeIsAtLeastU32, align_up_checked};

/// Parse a CBFS master header from a ROM image.
///
/// `rom_base` is the absolute address where the ROM is mapped (e.g.
/// `0xFFC00000` for a 4MB ROM). This is needed to resolve the header
/// pointer at the last 4 bytes of the ROM, which stores an absolute address.
///
/// Returns the parsed header and the entries alignment/offset on success.
pub(crate) fn parse_header(rom: &[u8], rom_base: u32) -> Result<(CbfsHeader, u32, u32), CbfsError> {
    // Verify ROM size and ROM base address
    let rom_size: u32 = rom
        .len()
        .try_into()
        .ok()
        .filter(|s: &u32| s.is_power_of_two())
        .ok_or(CbfsError::InvalidRomSize)?;

    if !rom_base.is_multiple_of(rom_size) {
        return Err(CbfsError::InvalidRomBase);
    }

    // Read header pointer from last 4 bytes of ROM (little-endian absolute address)
    let (_, ptr) = U32::<LE>::read_from_suffix(rom).map_err(|_| CbfsError::InvalidRomSize)?;

    // Then subtract the ROM base address to get the header offset and read the header
    let header_off = ptr
        .get()
        .checked_sub(rom_base)
        .ok_or(CbfsError::InvalidHeaderPointer)?;
    let (header, _) = rom
        .get(header_off.as_usize()..)
        .and_then(|s| CbfsHeader::read_from_prefix(s).ok())
        .ok_or(CbfsError::InvalidHeaderPointer)?;

    // Verify header fields
    if header.magic.get() != CBFS_HEADER_MAGIC
        || header.version.get() != CBFS_HEADER_VERSION2
        || header.romsize.get() != rom_size
        || !header.align.get().is_power_of_two()
        || header.align > rom_size
        || header.offset > rom_size
    {
        return Err(CbfsError::InvalidHeader);
    }

    let align = header.align.get();
    let offset = header.offset.get();

    Ok((header, align, offset))
}

/// Positional information about a parsed CBFS entry.
///
/// All positions are absolute ROM offsets stored as `u32`, matching the
/// on-flash format. Use [`.as_usize()`](crate::UsizeIsAtLeastU32) when
/// indexing into a ROM buffer. Contains only scalars (no references
/// into the ROM), so the borrow used to produce this value is released
/// immediately and callers can mutate the ROM between calls to
/// [`CbfsEntryWalker::next`].
pub(crate) struct CbfsEntryInfo {
    /// Start of this entry's header in the ROM.
    pub entry_start: u32,
    /// End of this entry's data region in the ROM.
    pub entry_end: u32,
    /// Entry type field.
    pub entry_type: u32,
    /// Start of the file data in the ROM.
    pub data_start: u32,
    /// Length of data.
    pub data_len: u32,
    /// Start of the NUL-terminated name in the ROM.
    pub name_start: u32,
    /// Length of the name (excluding NUL).
    pub name_len: u32,
}

/// Iterator-like walker over CBFS entries.
///
/// Borrows the ROM only during each [`next`](Self::next) call, allowing the
/// caller to mutate the ROM between iterations.
pub(crate) struct CbfsEntryWalker {
    align: u32,
    cursor: Option<u32>,
}

impl CbfsEntryWalker {
    pub fn new(align: u32, offset: u32) -> Self {
        Self {
            align,
            cursor: Some(offset),
        }
    }

    /// Parse the entry at the current cursor and advance.
    ///
    /// Returns `None` when there are no more entries (end of ROM or no
    /// valid magic). Returns `Some(Err(CbfsError::CorruptEntry))` when
    /// the entry has valid magic but invalid metadata; iteration is
    /// terminated in that case.
    pub fn next(&mut self, rom: &[u8]) -> Option<Result<CbfsEntryInfo, CbfsError>> {
        let cursor = self.cursor?;
        let rom_len: u32 = rom.len().try_into().ok()?;

        let hdr_size = crate::const_u32(size_of::<CbfsFileHeader>());

        // No magic match → end of iteration (None), not an error.
        let (hdr, _) = rom
            .get(cursor.as_usize()..)
            .and_then(|s| CbfsFileHeader::read_from_prefix(s).ok())
            .filter(|(hdr, _)| hdr.magic == CBFS_FILE_MAGIC)?;

        // Magic matched — from here on, failures are corruption.
        let result = (|| {
            let data_offset = hdr.offset.get();
            if data_offset < hdr_size
                || data_offset > rom_len.checked_sub(cursor).ok_or(CbfsError::CorruptEntry)?
            {
                return Err(CbfsError::CorruptEntry);
            }

            let data_len = hdr.len.get();
            if data_len > rom_len - cursor - data_offset {
                return Err(CbfsError::CorruptEntry);
            }

            let name_start = cursor + hdr_size;
            let name_end = cursor + data_offset;
            let name_len: u32 = rom[name_start.as_usize()..name_end.as_usize()]
                .iter()
                .position(|&b| b == 0)
                .ok_or(CbfsError::CorruptEntry)?
                .try_into()
                .map_err(|_| CbfsError::CorruptEntry)?;

            let data_start = cursor + data_offset;
            let entry_end = data_start + data_len;

            Ok(CbfsEntryInfo {
                entry_start: cursor,
                entry_end,
                entry_type: hdr.r#type.get(),
                data_start,
                data_len,
                name_start,
                name_len,
            })
        })();

        match &result {
            Ok(info) => self.cursor = align_up_checked!(info.entry_end, self.align),
            Err(_) => self.cursor = None,
        }

        Some(result)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::cbfs::writer::CbfsWriter;
    use crate::const_u32;
    use zerocopy::IntoBytes;

    const ROM_SIZE: u32 = 4096;
    const ROM_BASE: u32 = 0xFFFF_F000;

    fn valid_rom() -> Vec<u8> {
        let mut rom = vec![0xFFu8; ROM_SIZE.as_usize()];
        CbfsWriter::create(&mut rom, ROM_BASE, CBFS_DEFAULT_ALIGNMENT).unwrap();
        rom
    }

    /// Position of the first entry after the master header.
    fn second_entry_pos() -> usize {
        let mh_total = size_of::<CbfsFileHeader>() + b"cbfs_master_header\0".len();
        crate::align_up!(
            mh_total + size_of::<CbfsHeader>(),
            CBFS_DEFAULT_ALIGNMENT.as_usize()
        )
    }

    fn rom_with_header_field(mutate: impl FnOnce(&mut CbfsHeader)) -> Vec<u8> {
        let mut hdr = CbfsHeader {
            magic: CBFS_HEADER_MAGIC.into(),
            version: CBFS_HEADER_VERSION2.into(),
            romsize: ROM_SIZE.into(),
            bootblocksize: 4.into(),
            align: CBFS_DEFAULT_ALIGNMENT.into(),
            offset: 0.into(),
            architecture: CBFS_ARCHITECTURE_X86.into(),
            _pad: 0.into(),
        };
        mutate(&mut hdr);
        let mut rom = vec![0u8; ROM_SIZE.as_usize()];
        rom[..size_of::<CbfsHeader>()].copy_from_slice(hdr.as_bytes());
        rom[ROM_SIZE.as_usize() - 4..].copy_from_slice(&ROM_BASE.to_le_bytes());
        rom
    }

    fn corrupt_entry(offset_val: u32, len_val: u32) -> CbfsFileHeader {
        CbfsFileHeader {
            magic: CBFS_FILE_MAGIC,
            len: len_val.into(),
            r#type: CBFS_TYPE_RAW.into(),
            attributes_offset: 0.into(),
            offset: offset_val.into(),
        }
    }

    // --- parse_header ---

    #[test]
    fn test_parse_header_valid() {
        let rom = valid_rom();
        let (hdr, align, offset) = parse_header(&rom, ROM_BASE).unwrap();
        assert_eq!(hdr.magic.get(), CBFS_HEADER_MAGIC);
        assert_eq!(align, CBFS_DEFAULT_ALIGNMENT);
        assert_eq!(offset, 0);
    }

    #[test]
    fn test_parse_header_rejects_invalid() {
        // Empty ROM
        assert!(parse_header(&[], 0).is_err());
        // Pointer below ROM base
        let mut rom = vec![0u8; ROM_SIZE.as_usize()];
        rom[ROM_SIZE.as_usize() - 4..].copy_from_slice(&(ROM_BASE - 1).to_le_bytes());
        assert!(parse_header(&rom, ROM_BASE).is_err());
        // Bad magic (zeros at header offset)
        let rom = rom_with_header_field(|h| h.magic = 0.into());
        assert!(parse_header(&rom, ROM_BASE).is_err());
        // Bad version
        let rom = rom_with_header_field(|h| h.version = 0xDEAD.into());
        assert!(parse_header(&rom, ROM_BASE).is_err());
        // Mismatched romsize
        let rom = rom_with_header_field(|h| h.romsize = (ROM_SIZE + 1).into());
        assert!(parse_header(&rom, ROM_BASE).is_err());
        // Non-power-of-two alignment
        let rom = rom_with_header_field(|h| h.align = 3.into());
        assert!(parse_header(&rom, ROM_BASE).is_err());
        // Offset past ROM
        let rom = rom_with_header_field(|h| h.offset = (ROM_SIZE + 1).into());
        assert!(parse_header(&rom, ROM_BASE).is_err());
    }

    // --- CbfsEntryWalker ---

    #[test]
    fn test_walker_valid_image() {
        let rom = valid_rom();
        let (_, align, offset) = parse_header(&rom, ROM_BASE).unwrap();
        let mut w = CbfsEntryWalker::new(align, offset);

        let e = w.next(&rom).unwrap().unwrap();
        assert_eq!(e.entry_type, CBFS_TYPE_RAW);
        assert_eq!(
            &rom[e.name_start.as_usize()..(e.name_start + e.name_len).as_usize()],
            b"cbfs_master_header"
        );

        let e = w.next(&rom).unwrap().unwrap();
        assert_eq!(e.entry_type, CBFS_TYPE_NULL);

        assert!(w.next(&rom).is_none());
    }

    #[test]
    fn test_walker_stops_on_corrupt_entry() {
        let cases: &[(u32, u32)] = &[
            (0, 0),                                                 // offset = 0
            (8, 0),                                                 // offset < header size
            (ROM_SIZE, 0),                                          // offset past ROM
            (const_u32(size_of::<CbfsFileHeader>()) + 1, ROM_SIZE), // len past ROM
        ];
        let pos = second_entry_pos();

        for &(off, len) in cases {
            let mut rom = valid_rom();
            let entry = corrupt_entry(off, len);
            rom[pos..pos + size_of::<CbfsFileHeader>()].copy_from_slice(entry.as_bytes());

            let (_, align, offset) = parse_header(&rom, ROM_BASE).unwrap();
            let mut w = CbfsEntryWalker::new(align, offset);
            w.next(&rom); // master header
            assert!(
                matches!(w.next(&rom), Some(Err(CbfsError::CorruptEntry))),
                "expected CorruptEntry for offset={off}, len={len}"
            );
        }
    }

    #[test]
    fn test_walker_stops_on_missing_nul() {
        let mut rom = valid_rom();
        let pos = second_entry_pos();
        let data_offset = size_of::<CbfsFileHeader>() + 4;
        let entry = corrupt_entry(const_u32(data_offset), 0);
        rom[pos..pos + size_of::<CbfsFileHeader>()].copy_from_slice(entry.as_bytes());
        rom[pos + size_of::<CbfsFileHeader>()..pos + data_offset].fill(b'A');

        let (_, align, offset) = parse_header(&rom, ROM_BASE).unwrap();
        let mut w = CbfsEntryWalker::new(align, offset);
        w.next(&rom);
        assert!(matches!(w.next(&rom), Some(Err(CbfsError::CorruptEntry))));
    }
}

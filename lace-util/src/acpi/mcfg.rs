// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! ACPI MCFG (Memory Mapped Configuration Space) table parser

use super::SdtHeader;
use zerocopy::{
    FromBytes, Immutable, KnownLayout,
    byteorder::{LE, U16, U32, U64},
};

/// MCFG allocation entry — describes one PCI segment's ECAM region.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromBytes, Immutable, KnownLayout)]
pub struct McfgEntry {
    pub base_address: U64<LE>,
    pub segment_group: U16<LE>,
    pub start_bus: u8,
    pub end_bus: u8,
    pub _reserved: U32<LE>,
}

/// Parse the MCFG table from a byte slice and return the first allocation entry.
///
/// The caller is responsible for producing `data` from the physical MCFG
/// address; by taking a slice rather than a raw pointer the parsing logic is
/// safe and unit-testable.
pub fn parse_mcfg(data: &[u8]) -> Option<McfgEntry> {
    let (header, _) = SdtHeader::ref_from_prefix(data).ok()?;
    if &header.signature != b"MCFG" {
        return None;
    }

    // 8 bytes of reserved space follow the SDT header, then the entries.
    let entries_offset = size_of::<SdtHeader>() + 8;
    let total_length = header.length.get().try_into().unwrap();
    if total_length > data.len() || total_length < entries_offset + size_of::<McfgEntry>() {
        return None;
    }

    let (entry, _) = McfgEntry::ref_from_prefix(&data[entries_offset..total_length]).ok()?;
    Some(*entry)
}

#[cfg(test)]
mod test {
    use super::*;

    fn build_mcfg(entry: &McfgEntry) -> alloc::vec::Vec<u8> {
        let sdt_hdr_size = size_of::<SdtHeader>();
        let entries_offset = sdt_hdr_size + 8;
        let total = entries_offset + size_of::<McfgEntry>();
        let mut buf = alloc::vec![0u8; total];
        buf[0..4].copy_from_slice(b"MCFG");
        buf[4..8].copy_from_slice(&(total as u32).to_le_bytes());

        let entry_bytes = &mut buf[entries_offset..total];
        entry_bytes[0..8].copy_from_slice(&entry.base_address.get().to_le_bytes());
        entry_bytes[8..10].copy_from_slice(&entry.segment_group.get().to_le_bytes());
        entry_bytes[10] = entry.start_bus;
        entry_bytes[11] = entry.end_bus;
        buf
    }

    #[test]
    fn test_parse_mcfg_valid() {
        let entry = McfgEntry {
            base_address: 0xE000_0000.into(),
            segment_group: 0.into(),
            start_bus: 0,
            end_bus: 0xFF,
            _reserved: 0.into(),
        };
        let buf = build_mcfg(&entry);
        let parsed = parse_mcfg(&buf).unwrap();
        assert_eq!({ parsed.base_address }, 0xE000_0000);
        assert_eq!({ parsed.segment_group }, 0);
        assert_eq!(parsed.start_bus, 0);
        assert_eq!(parsed.end_bus, 0xFF);
    }

    #[test]
    fn test_parse_mcfg_bad_signature() {
        let entry = McfgEntry {
            base_address: 0.into(),
            segment_group: 0.into(),
            start_bus: 0,
            end_bus: 0,
            _reserved: 0.into(),
        };
        let mut buf = build_mcfg(&entry);
        buf[0..4].copy_from_slice(b"FACP");
        assert!(parse_mcfg(&buf).is_none());
    }

    #[test]
    fn test_parse_mcfg_truncated() {
        let entry = McfgEntry {
            base_address: 0.into(),
            segment_group: 0.into(),
            start_bus: 0,
            end_bus: 0,
            _reserved: 0.into(),
        };
        let buf = build_mcfg(&entry);
        // Drop the last byte of the entry — length field still claims full size.
        assert!(parse_mcfg(&buf[..buf.len() - 1]).is_none());
    }

    #[test]
    fn test_parse_mcfg_no_entries() {
        let sdt_hdr_size = size_of::<SdtHeader>();
        let total = sdt_hdr_size + 8; // header + reserved, no entries
        let mut buf = alloc::vec![0u8; total];
        buf[0..4].copy_from_slice(b"MCFG");
        buf[4..8].copy_from_slice(&(total as u32).to_le_bytes());
        assert!(parse_mcfg(&buf).is_none());
    }
}

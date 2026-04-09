// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! ACPI MCFG (Memory Mapped Configuration Space) table parser

use super::SdtHeader;
use zerocopy::{FromBytes, Immutable, KnownLayout};

/// MCFG allocation entry — describes one PCI segment's ECAM region.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, FromBytes, Immutable, KnownLayout)]
pub struct McfgEntry {
    pub base_address: u64,
    pub segment_group: u16,
    pub start_bus: u8,
    pub end_bus: u8,
    pub _reserved: u32,
}

/// Parse the MCFG table and return the ECAM base address for the first segment.
///
/// # Safety
/// `mcfg_addr` must point to valid, identity-mapped ACPI MCFG table memory.
pub unsafe fn parse_mcfg(mcfg_addr: u64) -> Option<McfgEntry> {
    let ptr = mcfg_addr as *const u8;
    let header_bytes = unsafe { core::slice::from_raw_parts(ptr, size_of::<SdtHeader>()) };
    let (header, _) = SdtHeader::ref_from_prefix(header_bytes).ok()?;

    if &header.signature != b"MCFG" {
        return None;
    }

    // MCFG has 8 bytes of reserved space after the header, then entries
    let entries_offset = size_of::<SdtHeader>() + 8;
    let total_length = header.length as usize;
    if total_length < entries_offset + size_of::<McfgEntry>() {
        return None;
    }

    let entry_ptr = unsafe { ptr.add(entries_offset) };
    let entry_bytes = unsafe { core::slice::from_raw_parts(entry_ptr, size_of::<McfgEntry>()) };
    let (entry, _) = McfgEntry::ref_from_prefix(entry_bytes).ok()?;

    Some(*entry)
}

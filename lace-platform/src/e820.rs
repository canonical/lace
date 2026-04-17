// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! E820 memory map types.
//!
//! The E820 memory map is a standard for describing the physical memory layout
//! of a system. The minimum entry size is 20 bytes (base, length, type); some
//! sources (e.g. BIOS INT 15h) may return 24-byte entries with an additional
//! ACPI extended attributes field, which is ignored here.

use lace_util::Display;
use lace_util_derive::NumEnum;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// E820 memory map entry (20-byte wire format).
#[repr(C, packed)]
#[derive(Debug, Default, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct E820Entry {
    pub base: u64,
    pub length: u64,
    pub type_: u32,
}

/// Known E820 memory types. Values outside this set preserve their raw u32
/// encoding when round-tripping through the platform memory map.
#[derive(Debug, Display, NumEnum, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum E820MemoryType {
    #[display("Usable")]
    Usable = 1,
    #[display("Reserved")]
    Reserved = 2,
    #[display("ACPI Reclaim")]
    AcpiReclaim = 3,
    #[display("ACPI NVS")]
    AcpiNvs = 4,
    #[display("Bad Memory")]
    BadMemory = 5,
}

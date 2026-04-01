// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

use super::int::{BiosRegisters, bios_call};
use lace_util::Display;

#[repr(C, packed)]
#[derive(Debug, Default, Clone, Copy)]
pub struct E820Entry {
    pub base: u64,
    pub length: u64,
    pub type_: u32,
    pub acpi: u32,
}

impl E820Entry {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Debug, Display, Clone, Copy, PartialEq, Eq)]
pub enum E820MemoryType {
    #[display("Usable")]
    Usable,
    #[display("Reserved")]
    Reserved,
    #[display("ACPI Reclaim")]
    AcpiReclaim,
    #[display("ACPI NVS")]
    AcpiNvs,
    #[display("Bad Memory")]
    BadMemory,
    #[display("Unknown (0x{:X})")]
    Unknown(u32),
}

impl From<u32> for E820MemoryType {
    fn from(value: u32) -> Self {
        match value {
            1 => E820MemoryType::Usable,
            2 => E820MemoryType::Reserved,
            3 => E820MemoryType::AcpiReclaim,
            4 => E820MemoryType::AcpiNvs,
            5 => E820MemoryType::BadMemory,
            other => E820MemoryType::Unknown(other),
        }
    }
}

/// Get the system memory map using BIOS INT 15h E820h function.
/// The caller must provide a buffer in low memory (<1MB) to store the entries.
pub fn get_memory_map(entries: &mut [E820Entry]) -> usize {
    let addr = entries.as_ptr() as u64;
    if addr >= 0x100000 {
        panic!("Buffer must be in low memory (<1MB)");
    }

    let mut count = 0;
    let mut continuation = 0;
    let smap_sig = 0x534D4150; // 'SMAP'

    for entry in entries.iter_mut() {
        let mut regs = BiosRegisters::new();
        // EAX = 0xE820
        regs.eax = 0xE820;
        // EBX = Continuation
        regs.ebx = continuation;
        // ECX = Buffer size (20 bytes minimum, we use 24 for full struct including acpi)
        regs.ecx = 24;
        // EDX = 'SMAP'
        regs.edx = smap_sig;

        // ES:DI = Pointer to entry
        let ptr = entry as *mut E820Entry as u64;
        regs.es = (ptr >> 4) as u16;
        regs.edi = (ptr & 0xF) as u32;

        unsafe {
            bios_call(0x15, &mut regs);
        }

        // Check for error (Carry Flag)
        if (regs.flags & 1) != 0 {
            break;
        }

        // Check signature
        if regs.eax != smap_sig {
            break;
        }

        // If length is 0, ignore (some BIOSes do this)
        if entry.length > 0 {
            count += 1;
        }

        continuation = regs.ebx;
        if continuation == 0 {
            break;
        }
    }

    count
}

// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! x86 legacy PCI config space access (0xCF8/0xCFC)
//!
//! Only used for chipset bootstrap (e.g., enabling Q35 PCIEXBAR).

use super::port_io;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

fn config_addr(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xFC)
}

/// Read a 32-bit value from PCI config space.
///
/// # Safety
/// The caller must ensure the BDF and offset are valid.
pub unsafe fn read_u32(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    unsafe {
        port_io::outl(CONFIG_ADDRESS, config_addr(bus, dev, func, offset));
        port_io::inl(CONFIG_DATA)
    }
}

/// Write a 32-bit value to PCI config space.
///
/// # Safety
/// The caller must ensure the BDF and offset are valid.
pub unsafe fn write_u32(bus: u8, dev: u8, func: u8, offset: u8, value: u32) {
    unsafe {
        port_io::outl(CONFIG_ADDRESS, config_addr(bus, dev, func, offset));
        port_io::outl(CONFIG_DATA, value);
    }
}

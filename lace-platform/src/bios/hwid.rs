// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! BIOS hardware identification

use lace_util::fdt::Fdt;
use lace_util::smbios::{Smbios3EntryPoint, SmbiosEntryPoint};

/// Finds the EDID data.
pub fn find_edid() -> Option<impl core::ops::Deref<Target = [u8]>> {
    // TODO: Implement EDID finding via VBE (INT 10h AX=4F15h)
    None::<&[u8]>
}

/// Finds SMBIOS tables by scanning the legacy memory range 0xF0000-0xFFFFF.
pub fn find_smbios_tables() -> Option<(&'static [u8], &'static [u8])> {
    let start = 0xF0000;
    let end = 0xFFFFF;
    let step = 16;

    for addr in (start..=end).step_by(step) {
        let ptr = addr as *const u8;
        unsafe {
            // Check for _SM3_ (SMBIOS 3.0)
            if *ptr == b'_'
                && *ptr.add(1) == b'S'
                && *ptr.add(2) == b'M'
                && *ptr.add(3) == b'3'
                && *ptr.add(4) == b'_'
            {
                let entry = &*(ptr as *const Smbios3EntryPoint);
                // TODO: Verify checksum
                return Some((
                    core::slice::from_raw_parts(ptr, core::mem::size_of::<Smbios3EntryPoint>()),
                    core::slice::from_raw_parts(
                        entry.table_address as *const u8,
                        entry.table_maximum_size as usize,
                    ),
                ));
            }
            // Check for _SM_ (SMBIOS 2.1)
            if *ptr == b'_' && *ptr.add(1) == b'S' && *ptr.add(2) == b'M' && *ptr.add(3) == b'_' {
                let entry = &*(ptr as *const SmbiosEntryPoint);
                // TODO: Verify checksum
                return Some((
                    core::slice::from_raw_parts(ptr, core::mem::size_of::<SmbiosEntryPoint>()),
                    core::slice::from_raw_parts(
                        entry.table_address as *const u8,
                        entry.table_length as usize,
                    ),
                ));
            }
        }
    }
    None
}

/// Finds an installed DTB in the system.
/// # Safety
/// This is not implemented for now.
pub unsafe fn find_dtb() -> Option<Fdt<'static>> {
    todo!()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DtbReceipt;

/// Installs a DTB in the system.
/// # Safety
/// This is not implemented for now.
pub unsafe fn install_dtb(_dtb: &[u8]) -> Result<DtbReceipt, super::Error> {
    todo!()
}

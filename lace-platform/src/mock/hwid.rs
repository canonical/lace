// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Mock hardware identification

pub fn find_edid() -> Option<impl std::ops::Deref<Target = [u8]>> {
    None::<&[u8]>
}

pub fn find_smbios_tables() -> Option<(&'static [u8], &'static [u8])> {
    None
}

/// Finds an installed DTB in the system.
/// # Safety
/// This is not implemented for now.
pub unsafe fn find_dtb() -> Option<lace_util::fdt::Fdt<'static>> {
    None
}

/// Placeholder for a DTB installation receipt.
pub struct MockDtbReceipt;

/// Installs a DTB in the system.
/// # Safety
/// This is not implemented for now.
pub unsafe fn install_dtb(_dtb: &[u8]) -> Result<MockDtbReceipt, super::Error> {
    Err(super::Error)
}

// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Virt platform hardware identification stubs.

use lace_util::fdt::Fdt;

pub fn find_edid() -> Option<impl core::ops::Deref<Target = [u8]>> {
    None::<&[u8]>
}

pub fn find_smbios_tables() -> Option<(&'static [u8], &'static [u8])> {
    // TODO: read from fw_cfg etc/smbios/*
    None
}

/// # Safety
/// Not implemented for virt platform.
pub unsafe fn find_dtb() -> Option<Fdt<'static>> {
    None
}

pub struct DtbReceipt;

/// # Safety
/// Not implemented for virt platform.
pub unsafe fn install_dtb(_dtb: &[u8]) -> Result<DtbReceipt, super::Error> {
    todo!()
}

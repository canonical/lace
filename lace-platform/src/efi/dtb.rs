// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Device tree related EFI functionality.

use crate::efi::mem::{
    MemoryType, PageAllocation, PageAllocationConstraint, PageAllocationIface, page_count,
};
use crate::efi::open_protocol_exclusive;
use crate::efi::proto::dt_fixup;

use lace_util::fdt::Fdt;

/// EFI configuration table pointing to a device tree.
/// The DTB must be contained in memory of type EfiACPIReclaimMemory.
/// Ref: 4.6.1.3. Devicetree Tables
pub const DTB_TABLE_GUID: uefi::Guid = uefi::guid!("b1b621d5-f19c-41a5-830b-d9152c69aae0");

/// A receipt for an installed device tree configuration table.
/// When dropped, the configuration table is removed from the EFI system table,
/// and the memory holding the table is freed safely.
struct DtbReceipt {
    guid: &'static uefi::Guid,
    _buf: PageAllocation,
}

impl Drop for DtbReceipt {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: We are removing the configuration table that was installed
            // when this receipt was created.
            // After this the page allocation holding the table can be dropped safely.
            let _ = uefi::boot::install_configuration_table(self.guid, core::ptr::null());
        }
    }
}

/// Install the given DTB as a configuration table into the EFI system table.
/// The returned `DtbReceipt` will own the DTB, ensuring the configuration table
/// is removed if and only if the receipt is dropped.
/// # Safety
/// This function can be used to invalidate an existing DTB configuration table,
/// which other code might have references to. The caller must ensure that
/// no such references exist.
pub unsafe fn install_dtb(dtb: &[u8]) -> Result<impl Drop, uefi::Error> {
    let buf: PageAllocation = match open_protocol_exclusive::<dt_fixup::DtFixupProtocol>() {
        Ok(mut proto) => proto.fixup_owned(dtb)?,
        Err(_) => PageAllocation::new_init_prefix(
            PageAllocationConstraint::AnyAddress,
            Some(MemoryType::ACPI_RECLAIM),
            page_count(dtb.len()),
            None,
            dtb,
        )?,
    };
    unsafe {
        // SAFETY: table_ptr is valid for the lifetime of the returned DtbReceipt.
        // When DtbReceipt is dropped, the DTB entry is removed from the system table,
        // and only then is the memory freed.
        uefi::boot::install_configuration_table(
            &DTB_TABLE_GUID,
            buf.as_ptr() as *const core::ffi::c_void,
        )?
    };
    Ok(DtbReceipt {
        guid: &DTB_TABLE_GUID,
        _buf: buf,
    })
}

/// Find the installed DTB from the EFI system table.
/// Returns `None` if no DTB configuration table is installed (or if the DTB is invalid).
/// # Safety
/// The returned `Fdt` is valid as long as the DTB configuration table remains installed
/// in the EFI system table. The caller must ensure this is the case.
pub unsafe fn find_dtb() -> Option<Fdt<'static>> {
    let ptr: *const u8 = super::find_config_table(DTB_TABLE_GUID)?;
    unsafe {
        // SAFETY: The pointer from the configuration table is valid as long as the table is installed,
        // which is guaranteed by the caller.
        Fdt::from_ptr(ptr).ok()
    }
}

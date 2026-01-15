// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! EFI device-tree fix-up protocol.

use crate::efi::mem::{
    MemoryType, PageAllocation, PageAllocationConstraint, PageAllocationIface, page_count,
};

/// The EFI device-tree fix-up protocol provides a function to let the firmware apply fix-ups.
/// Ref: https://github.com/U-Boot-EFI/EFI_DT_FIXUP_PROTOCOL
pub struct DtFixupProtocol {
    pub revision: u64,
    pub fixup: unsafe extern "efiapi" fn(
        *mut DtFixupProtocol,
        fdt: *mut core::ffi::c_void,
        buffer_size: *mut usize,
        flags: u32,
    ) -> uefi::Status,
}

/// Add nodes and update properties
const DT_APPLY_FIXUPS: u32 = 0x00000001;

/// Reserve memory according to the /reserved-memory node and the memory reservation block
const EFI_DT_RESERVE_MEMORY: u32 = 0x00000002;

unsafe impl uefi::Identify for DtFixupProtocol {
    const GUID: uefi::Guid = uefi::guid!("e617d64c-fe08-46da-f4dc-bbd5870c7300");
}

impl uefi::proto::Protocol for DtFixupProtocol {}

impl DtFixupProtocol {
    /// Apply fix-ups directly into the provided DTB buffer.
    /// On success, returns Ok(()). If the buffer is too small, returns Err with the required size.
    pub fn fixup(&mut self, dtb_buffer: &mut [u8]) -> uefi::Result<(), usize> {
        let mut buffer_size = dtb_buffer.len();
        let status = unsafe {
            // SAFETY: buffer.as_mut_ptr() is valid for buffer_size bytes
            (self.fixup)(
                self,
                dtb_buffer.as_mut_ptr() as *mut core::ffi::c_void,
                &mut buffer_size,
                DT_APPLY_FIXUPS | EFI_DT_RESERVE_MEMORY,
            )
        };
        match status {
            uefi::Status::SUCCESS => Ok(()),
            uefi::Status::BUFFER_TOO_SMALL => Err(uefi::Error::new(status, buffer_size)),
            _ => Err(uefi::Error::new(status, 0)),
        }
    }

    /// Apply fix-ups into a newly allocated buffer, returning ownership of the buffer.
    pub fn fixup_owned(&mut self, dtb: &[u8]) -> uefi::Result<PageAllocation> {
        // Create buffer holding a copy of the dtb
        let mut buf = PageAllocation::new_init_prefix(
            PageAllocationConstraint::AnyAddress,
            MemoryType::ACPI_RECLAIM,
            page_count(dtb.len()),
            dtb,
        )?;
        // Try to apply fixups, but we might get BUFFER_TOO_SMALL on
        // the first attempt
        match self.fixup(buf.as_u8_slice_mut()) {
            Ok(()) => {
                // Successfully applied fix-ups
                Ok(buf)
            }
            Err(e) if e.status() == uefi::Status::BUFFER_TOO_SMALL => {
                // Reallocate with correct size
                buf = PageAllocation::new_init_prefix(
                    PageAllocationConstraint::AnyAddress,
                    MemoryType::ACPI_RECLAIM,
                    page_count(*e.data()),
                    dtb,
                )?;
                // We will not get BUFFER_TOO_SMALL this time
                self.fixup(buf.as_u8_slice_mut())
                    .map(|_| buf)
                    .map_err(|e| e.to_err_without_payload())
            }
            Err(e) => {
                // Any other error
                Err(e.to_err_without_payload())
            }
        }
    }
}

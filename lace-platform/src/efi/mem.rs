// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! UEFI memory management utilities.

use crate::mem::{MemAttributes, PageAllocationConstraint, PageAllocationIface};
use core::ptr::NonNull;
use lace_util::peimage;
use uefi::boot::ScopedProtocol;

/// Type alias for UEFI physical addresses.
pub type Address = u64;

/// Page size used by the UEFI boot services page allocator.
pub const PAGE_SIZE: usize = uefi::boot::PAGE_SIZE;

/// Type alias for UEFI boot services page allocation types.
pub type AllocateType = uefi::boot::AllocateType;

/// Type alias for UEFI boot services memory types.
pub type MemoryType = uefi::boot::MemoryType;

/// Conversion from UEFI boot services page allocation types to generic page allocation constraints.
impl From<PageAllocationConstraint<Address>> for uefi::boot::AllocateType {
    fn from(value: PageAllocationConstraint<Address>) -> Self {
        match value {
            PageAllocationConstraint::AnyAddress => uefi::boot::AllocateType::AnyPages,
            PageAllocationConstraint::MaxAddress(addr) => {
                uefi::boot::AllocateType::MaxAddress(addr)
            }
            PageAllocationConstraint::FixedAddress(addr) => uefi::boot::AllocateType::Address(addr),
        }
    }
}

/// Resource holder for an allocation from the UEFI boot services page allocator.
pub struct PageAllocation {
    ptr: NonNull<u8>,
    pages: usize,
}

impl PageAllocationIface<Address> for PageAllocation {
    const PAGE_SIZE: usize = PAGE_SIZE;

    type MemoryType = MemoryType;

    type Error = uefi::Error;

    unsafe fn new_uninit(
        constraint: PageAllocationConstraint<Address>,
        memory_type: Option<MemoryType>,
        pages: usize,
        alignment: Option<usize>,
    ) -> Result<Self, uefi::Error> {
        // UEFI boot services only guarantee page-aligned memory.
        // Reject requests for larger alignment.
        if let Some(align) = alignment
            && align > PAGE_SIZE
        {
            return Err(uefi::Error::new(uefi::Status::UNSUPPORTED, ()));
        }
        let memory_type = memory_type.unwrap_or(MemoryType::LOADER_DATA);
        let ptr = uefi::boot::allocate_pages(constraint.into(), memory_type, pages)?;
        Ok(PageAllocation { ptr, pages })
    }

    fn pages(&self) -> usize {
        self.pages
    }

    unsafe fn from_raw(ptr: NonNull<u8>, pages: usize) -> Self {
        PageAllocation { ptr, pages }
    }

    fn into_raw(self) -> (NonNull<u8>, usize) {
        let (ptr, pages) = (self.ptr, self.pages);
        core::mem::forget(self);
        (ptr, pages)
    }

    fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    fn as_u8_slice(&self) -> &[u8] {
        unsafe {
            // SAFETY: `ptr` was allocated with `boot::allocate_pages` and is valid for `pages` pages.
            // The resulting slice will have a lifetime tied to &self, so it cannot outlive the allocation.
            // The memory might be uninitialized, but any value of a byte is valid for u8.
            core::slice::from_raw_parts(self.ptr.as_ptr(), self.pages * PAGE_SIZE)
        }
    }

    fn as_u8_slice_mut(&mut self) -> &mut [u8] {
        unsafe {
            // SAFETY: `ptr` was allocated with `boot::allocate_pages` and is valid for `pages` pages.
            // The resulting slice will have a lifetime tied to &mut self, so it cannot outlive the allocation.
            // The memory might be uninitialized, but any value of a byte is valid for u8.
            core::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.pages * PAGE_SIZE)
        }
    }
}

impl Drop for PageAllocation {
    fn drop(&mut self) {
        // Set memory as RWX before freeing. This is a workaround for an EDK2 bug
        // where freeing memory with certain attributes set can cause a crash.
        // See: https://github.com/rhboot/shim/blob/c4665d282072df2ed8ab6ae1d5fa0de41e5db02f/loader-proto.c#L194
        crate::debugln!("WORKAROUND: Clearing memory attributes before freeing pages");
        let _ = change_mem_attrs(
            self.as_ptr() as u64..(self.as_ptr() as u64 + (self.pages * PAGE_SIZE) as u64),
            MemAttributes::empty(),
        );

        unsafe {
            // SAFETY: `ptr` was allocated with `uefi::boot::allocate_pages` and is valid for `pages` pages
            let _ = uefi::boot::free_pages(self.ptr, self.pages);
        }
    }
}

/// Change memory attributes for the given address range.
/// Note that this is best-effort, if the Memory Protection Protocol is not available,
/// this function will simply return without making any changes.
pub fn change_mem_attrs(
    addr_range: core::ops::Range<u64>,
    attrs: MemAttributes,
) -> Result<(), uefi::Error> {
    let Ok(mem_prot) =
        super::open_first_protocol_exclusive::<uefi::proto::security::MemoryProtection>()
    else {
        crate::debugln!(
            "EFI Memory Protection Protocol not available, cannot change memory attributes"
        );
        return Ok(());
    };
    let (set, clear) = lace_mem_attrs_to_uefi(&attrs);
    if !set.is_empty() {
        crate::debugln!(
            "Setting EFI memory attributes for range {:x}-{:x} to {:?}",
            addr_range.start,
            addr_range.end,
            set
        );
        mem_prot.set_memory_attributes(addr_range.clone(), set)?;
    }
    if !clear.is_empty() {
        crate::debugln!(
            "Clearing EFI memory attributes for range {:x}-{:x} to {:?}",
            addr_range.start,
            addr_range.end,
            clear
        );
        mem_prot.clear_memory_attributes(addr_range, clear)?;
    }
    Ok(())
}

/// Converts Lace MemAttributes to UEFI MemoryAttribute sets for setting and clearing.
fn lace_mem_attrs_to_uefi(
    attrs: &MemAttributes,
) -> (uefi::boot::MemoryAttribute, uefi::boot::MemoryAttribute) {
    let mut set_attrs = uefi::boot::MemoryAttribute::empty();
    let mut clear_attrs = uefi::boot::MemoryAttribute::empty();
    if attrs.contains(MemAttributes::READ_PROTECT) {
        set_attrs |= uefi::boot::MemoryAttribute::READ_PROTECT;
    } else {
        clear_attrs |= uefi::boot::MemoryAttribute::READ_PROTECT;
    }
    if attrs.contains(MemAttributes::WRITE_PROTECT) {
        set_attrs |= uefi::boot::MemoryAttribute::READ_ONLY;
    } else {
        clear_attrs |= uefi::boot::MemoryAttribute::READ_ONLY;
    }
    if attrs.contains(MemAttributes::EXECUTE_PROTECT) {
        set_attrs |= uefi::boot::MemoryAttribute::EXECUTE_PROTECT;
    } else {
        clear_attrs |= uefi::boot::MemoryAttribute::EXECUTE_PROTECT;
    }
    (set_attrs, clear_attrs)
}

/// Flag indicating whether NX is required by the firmware.
static NX_REQUIRED: spin::Once<bool> = spin::Once::new();

/// Returns whether NX is required by platform policy.
pub fn nx_required() -> bool {
    *NX_REQUIRED.get().unwrap()
}

/// Initialize UEFI memory management utilities.
pub(super) fn init() {
    // Determine if NX is required by the firmware.
    // We use the NX_COMPAT flag in the currently running image as a proxy.
    // 1. If an NX_COMPAT image is already running the firmware _might_ enforce NX thus we should too.
    //    Relaxing this to check if we are running on non-NX firmware could be useful,
    //    so that an NX image can chainload non-NX in that limited scenario, but
    //    that is not implemented currently. (And not required for the stubble use case.)
    // 2. If we are not currently running an NX_COMPAT image, then NX is obviously not required.
    let own_li: ScopedProtocol<uefi::proto::loaded_image::LoadedImage> =
        uefi::boot::open_protocol_exclusive(uefi::boot::image_handle())
            .expect("own loaded image protocol missing");
    let own_pe = peimage::parse_pe(unsafe {
        // SAFETY: this is valid in this function, because we exclusively hold the loaded image.
        let (base, len) = own_li.info();
        core::slice::from_raw_parts(base as *const u8, len as usize)
    })
    .expect("own PE image malformed");
    let nx_required = own_pe.nt_hdrs.optional_header.dll_characteristics
        & peimage::DLLCHARACTERISTICS_NX_COMPAT
        != 0;
    crate::debugln!("EFI memory: NX required = {}", nx_required);
    NX_REQUIRED.call_once(|| nx_required);
}

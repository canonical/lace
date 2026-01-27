// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! UEFI memory management utilities.

pub use crate::iface::mem::{PageAllocationConstraint, PageAllocationIface};
use core::ptr::NonNull;

/// Type alias for UEFI physical addresses.
pub type Address = u64;

/// Page size used by the UEFI boot services page allocator.
pub const PAGE_SIZE: usize = uefi::boot::PAGE_SIZE;

/// Computes the number of pages required to hold a given size in bytes,
/// rounding up to the nearest page.
pub const fn page_count(size: usize) -> usize {
    size.div_ceil(PAGE_SIZE)
}

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
        unsafe {
            // SAFETY: `ptr` was allocated with `uefi::boot::allocate_pages` and is valid for `pages` pages
            let _ = uefi::boot::free_pages(self.ptr, self.pages);
        }
    }
}

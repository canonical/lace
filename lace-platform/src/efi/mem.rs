// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! UEFI memory management utilities.

use core::ptr::NonNull;

/// Type alias for UEFI boot services page allocation types.
pub type AllocateType = uefi::boot::AllocateType;

/// Type alias for UEFI boot services memory types.
pub type MemoryType = uefi::boot::MemoryType;

/// Page size used by the UEFI boot services page allocator.
pub const PAGE_SIZE: usize = uefi::boot::PAGE_SIZE;

/// Macro to compute the number of pages required to hold a given size in bytes,
/// rounding up to the nearest page.
#[macro_export]
macro_rules! page_count {
    ($size:expr) => {
        $size.div_ceil($crate::efi::mem::PAGE_SIZE)
    };
}

pub use page_count;

/// Resource holder for an allocation from the UEFI boot services page allocator.
pub struct PageAllocation {
    ptr: NonNull<u8>,
    pages: usize,
}

impl PageAllocation {
    /// Allocates `pages` pages of memory of type `memory_type` using the UEFI boot services page allocator.
    /// The memory is uninitialized.
    pub fn new_uninit(
        allocate_type: AllocateType,
        memory_type: MemoryType,
        pages: usize,
    ) -> Result<Self, uefi::Error> {
        let ptr = uefi::boot::allocate_pages(allocate_type, memory_type, pages)?;
        Ok(PageAllocation { ptr, pages })
    }

    /// Allocates `pages` pages of memory of type `memory_type` using the UEFI boot services page allocator.
    /// The first init.len() bytes are initialized from the `init` slice, the rest is uninitialized.
    pub fn new_init_prefix(
        allocate_type: AllocateType,
        memory_type: MemoryType,
        pages: usize,
        init: &[u8],
    ) -> Result<Self, uefi::Error> {
        assert!(pages * PAGE_SIZE >= init.len());
        let mut pages = Self::new_uninit(allocate_type, memory_type, pages)?;
        pages.as_u8_slice_mut()[..init.len()].copy_from_slice(init);
        Ok(pages)
    }

    /// Returns the number of pages allocated.
    pub fn pages(&self) -> usize {
        self.pages
    }

    /// Create a PageAllocation from a raw pointer and page count.
    /// Dropping the PageAllocation will free the pages.
    /// # Safety
    /// The caller must ensure that the pointer was allocated with
    /// `boot::allocate_pages` and is valid for `pages` pages.
    pub unsafe fn from_raw(ptr: NonNull<u8>, pages: usize) -> Self {
        PageAllocation { ptr, pages }
    }

    /// Consumes the PageAllocation and returns the raw pointer and page count.
    /// The caller is responsible for freeing the pages.
    pub fn into_raw(self) -> (NonNull<u8>, usize) {
        let (ptr, pages) = (self.ptr, self.pages);
        core::mem::forget(self);
        (ptr, pages)
    }

    /// Returns the raw pointer to the allocated memory.
    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// Returns a slice to the allocated memory.
    pub fn as_u8_slice(&self) -> &[u8] {
        unsafe {
            // SAFETY: `ptr` was allocated with `boot::allocate_pages` and is valid for `pages` pages.
            // The resulting slice will have a lifetime tied to &self, so it cannot outlive the allocation.
            // The memory might be uninitialized, but any value of a byte is valid for u8.
            core::slice::from_raw_parts(self.ptr.as_ptr(), self.pages * PAGE_SIZE)
        }
    }

    /// Returns a mutable slice to the allocated memory.
    pub fn as_u8_slice_mut(&mut self) -> &mut [u8] {
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

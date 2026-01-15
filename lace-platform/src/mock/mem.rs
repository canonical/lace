// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Memory related sandbox platform abstractions.

use spin::Mutex;

use crate::iface::mem::{PageAllocationConstraint, PageAllocationIface};
use core::ptr::NonNull;

/// Address type for the sandbox platform.
pub type Address = usize;

/// Page size for the sandbox platform. (This is an arbitrary choice.)
pub const PAGE_SIZE: usize = 4096;

/// Computes the number of pages required to hold a given size in bytes,
/// rounding up to the nearest page.
pub const fn page_count(size: usize) -> usize {
    size.div_ceil(PAGE_SIZE)
}

/// Error type for page allocation failures in the mock platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageAllocationFailure {
    OutOfMemory,
    UnsupportedConstraint,
}

impl core::fmt::Display for PageAllocationFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PageAllocationFailure::OutOfMemory => write!(f, "Out of memory"),
            PageAllocationFailure::UnsupportedConstraint => {
                write!(f, "Unsupported page allocation constraint")
            }
        }
    }
}

/// Memory pool for the mock page allocator.
struct MockPagePool {
    memory: NonNull<u8>,
    size: usize,
    watermark: usize,
}

/// Safety: The memory pool is mutex protected, and overlapping allocations are not possible.
/// The initializer still has to ensure the underling memory is physically accessible from all threads,
/// but this is guaranteed on basically all hardware.
unsafe impl Send for MockPagePool {}

/// Global instance of the mock page pool.
static MOCK_PAGE_POOL: Mutex<Option<MockPagePool>> = Mutex::new(None);

/// Initializes the mock page pool with the given memory region.
/// # Safety
/// The caller must ensure that the provided memory region is valid for the lifetime of the program
/// and it will not be aliased anywhere else.
pub unsafe fn init_mock_page_pool(memory: NonNull<u8>, size: usize) {
    let mut guard = MOCK_PAGE_POOL.lock();
    if guard.is_some() {
        panic!("Mock page pool is already initialized");
    }
    *guard = Some(MockPagePool {
        memory,
        size,
        watermark: 0,
    });
}

/// Resource holder for an allocation from the mock page allocator.
pub struct PageAllocation {
    ptr: NonNull<u8>,
    pages: usize,
}

impl PageAllocationIface<Address> for PageAllocation {
    const PAGE_SIZE: usize = PAGE_SIZE;

    // No actual memory types in the mock platform.
    type MemoryType = ();

    type Error = PageAllocationFailure;

    unsafe fn new_uninit(
        constraint: PageAllocationConstraint<Address>,
        _memory_type: Self::MemoryType,
        pages: usize,
    ) -> Result<Self, Self::Error> {
        match constraint {
            PageAllocationConstraint::AnyAddress => (),
            _ => return Err(PageAllocationFailure::UnsupportedConstraint),
        }
        let mut guard = MOCK_PAGE_POOL.lock();

        let pool = guard.as_mut().expect("Mock page pool not initialized");

        let end: usize = pages
            .checked_mul(PAGE_SIZE)
            .and_then(|x| x.checked_add(pool.watermark))
            .ok_or(PageAllocationFailure::OutOfMemory)?;
        if end > pool.size {
            return Err(PageAllocationFailure::OutOfMemory);
        }

        let ptr = unsafe { pool.memory.as_ptr().add(pool.watermark) };
        pool.watermark = end;
        Ok(PageAllocation {
            ptr: NonNull::new(ptr).unwrap(),
            pages,
        })
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
        // In a real implementation, this would free the allocated pages.
        // Mock uses a bump allocator for now, so nothing to do here.
    }
}

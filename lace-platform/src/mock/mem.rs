// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Memory related sandbox platform abstractions.

use spin::Mutex;

use crate::iface::mem::{MemAttributes, PageAllocationConstraint, PageAllocationIface};
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

/// Default alignment when none is specified (page-aligned).
const DEFAULT_ALIGNMENT: usize = PAGE_SIZE;

/// Maximum supported alignment (4 MiB).
const MAX_ALIGNMENT: usize = 4 * 1024 * 1024;

/// Error type for page allocation failures in the mock platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageAllocationFailure {
    OutOfMemory,
    UnsupportedConstraint,
    /// The requested alignment is not a power of two.
    InvalidAlignment,
    /// The requested alignment exceeds the maximum (4 MiB).
    AlignmentTooLarge,
}

impl core::fmt::Display for PageAllocationFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OutOfMemory => write!(f, "Out of memory"),
            Self::UnsupportedConstraint => {
                write!(f, "Unsupported page allocation constraint")
            }
            Self::InvalidAlignment => {
                write!(f, "Alignment must be a power of two")
            }
            Self::AlignmentTooLarge => {
                write!(f, "Alignment exceeds maximum of 4 MiB")
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
        _memory_type: Option<Self::MemoryType>,
        pages: usize,
        alignment: Option<usize>,
    ) -> Result<Self, Self::Error> {
        match constraint {
            PageAllocationConstraint::AnyAddress => (),
            _ => return Err(PageAllocationFailure::UnsupportedConstraint),
        }

        // Default to page-aligned; validate alignment
        let alignment = alignment.unwrap_or(DEFAULT_ALIGNMENT);
        if !alignment.is_power_of_two() {
            return Err(PageAllocationFailure::InvalidAlignment);
        }
        if alignment > MAX_ALIGNMENT {
            return Err(PageAllocationFailure::AlignmentTooLarge);
        }

        let mut guard = MOCK_PAGE_POOL.lock();
        let pool = guard.as_mut().expect("Mock page pool not initialized");

        // Calculate aligned address
        let base_addr = pool.memory.as_ptr() as usize + pool.watermark;
        let aligned_addr = base_addr.next_multiple_of(alignment);
        let alignment_padding = aligned_addr - base_addr;

        // Calculate total size needed
        let alloc_size = pages
            .checked_mul(PAGE_SIZE)
            .ok_or(PageAllocationFailure::OutOfMemory)?;
        let total_size = alloc_size
            .checked_add(alignment_padding)
            .ok_or(PageAllocationFailure::OutOfMemory)?;
        let end = pool
            .watermark
            .checked_add(total_size)
            .ok_or(PageAllocationFailure::OutOfMemory)?;

        if end > pool.size {
            return Err(PageAllocationFailure::OutOfMemory);
        }

        pool.watermark = end;
        Ok(PageAllocation {
            ptr: NonNull::new(aligned_addr as *mut u8).unwrap(),
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

pub fn change_mem_attrs(
    _addr_range: core::ops::Range<u64>,
    _attrs: MemAttributes,
) -> Result<(), crate::Error> {
    // No-op in the mock platform.
    Ok(())
}

pub fn nx_required() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests share the global pool and must run serially (--test-threads=1).

    /// Initialize a fresh pool for testing.
    fn init_test_pool(pool: &mut [u8]) {
        let ptr = NonNull::new(pool.as_mut_ptr()).unwrap();
        *MOCK_PAGE_POOL.lock() = Some(MockPagePool {
            memory: ptr,
            size: pool.len(),
            watermark: 0,
        });
    }

    #[test]
    fn test_basic_allocation() {
        let mut pool = [0u8; 16 * PAGE_SIZE];
        init_test_pool(&mut pool);

        let alloc = unsafe {
            PageAllocation::new_uninit(PageAllocationConstraint::AnyAddress, None, 1, None)
        };
        assert!(alloc.is_ok());
        let alloc = alloc.unwrap();
        assert_eq!(alloc.pages(), 1);
    }

    #[test]
    fn test_zeroed_allocation() {
        let mut pool = [0xffu8; 16 * PAGE_SIZE];
        init_test_pool(&mut pool);

        let alloc = PageAllocation::new_zeroed(PageAllocationConstraint::AnyAddress, None, 1, None);
        assert!(alloc.is_ok());
        let alloc = alloc.unwrap();

        // Verify all bytes are zero
        assert!(alloc.as_u8_slice().iter().all(|&b| b == 0));
    }

    #[test]
    fn test_alignment() {
        let mut pool = [0u8; 64 * PAGE_SIZE];
        init_test_pool(&mut pool);

        // First allocation to move watermark
        let _ = unsafe {
            PageAllocation::new_uninit(PageAllocationConstraint::AnyAddress, None, 1, None)
        };

        // Request 64K alignment (16 pages)
        let alloc = unsafe {
            PageAllocation::new_uninit(
                PageAllocationConstraint::AnyAddress,
                None,
                1,
                Some(64 * 1024),
            )
        };
        assert!(alloc.is_ok());
        let alloc = alloc.unwrap();

        // Verify alignment
        let addr = alloc.as_ptr() as usize;
        assert_eq!(addr % (64 * 1024), 0, "allocation should be 64K aligned");
    }

    #[test]
    fn test_out_of_memory() {
        let mut pool = [0u8; PAGE_SIZE];
        init_test_pool(&mut pool);

        // Try to allocate more than the pool size
        let alloc = unsafe {
            PageAllocation::new_uninit(PageAllocationConstraint::AnyAddress, None, 2, None)
        };
        assert_eq!(alloc.err(), Some(PageAllocationFailure::OutOfMemory));
    }

    #[test]
    fn test_invalid_alignment() {
        let mut pool = [0u8; 16 * PAGE_SIZE];
        init_test_pool(&mut pool);

        // Alignment must be power of two
        let alloc = unsafe {
            PageAllocation::new_uninit(PageAllocationConstraint::AnyAddress, None, 1, Some(3))
        };
        assert_eq!(alloc.err(), Some(PageAllocationFailure::InvalidAlignment));
    }

    #[test]
    fn test_alignment_too_large() {
        let mut pool = [0u8; 16 * PAGE_SIZE];
        init_test_pool(&mut pool);

        // Alignment exceeds 4 MiB limit
        let alloc = unsafe {
            PageAllocation::new_uninit(
                PageAllocationConstraint::AnyAddress,
                None,
                1,
                Some(8 * 1024 * 1024),
            )
        };
        assert_eq!(alloc.err(), Some(PageAllocationFailure::AlignmentTooLarge));
    }

    #[test]
    fn test_unsupported_constraint() {
        let mut pool = [0u8; 16 * PAGE_SIZE];
        init_test_pool(&mut pool);

        let alloc = unsafe {
            PageAllocation::new_uninit(PageAllocationConstraint::MaxAddress(0x1000), None, 1, None)
        };
        assert_eq!(
            alloc.err(),
            Some(PageAllocationFailure::UnsupportedConstraint)
        );
    }
}

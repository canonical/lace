// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Memory related sandbox platform abstractions.

use lace_util::Display;
use spin::Mutex;

use crate::mem::{MemAttributes, PageAllocationConstraint, PageAllocationIface};
use core::ptr::NonNull;

/// Address type for the sandbox platform.
pub type Address = usize;

/// Page size for the sandbox platform. (This is an arbitrary choice.)
pub const PAGE_SIZE: usize = 4096;

/// Mock platform does not distinguish memory types
pub struct MemoryType;

/// Default alignment when none is specified (page-aligned).
const DEFAULT_ALIGNMENT: usize = PAGE_SIZE;

/// Maximum supported alignment (4 MiB).
const MAX_ALIGNMENT: usize = 4 * 1024 * 1024;

/// Error type for page allocation failures in the mock platform.
#[derive(Clone, Copy, Debug, Display, PartialEq, Eq)]
pub enum PageAllocationFailure {
    #[display("Out of memory")]
    OutOfMemory,
    #[display("Unsupported page allocation constraint")]
    UnsupportedConstraint,
    /// The requested alignment is not a power of two.
    #[display("Alignment must be a power of two")]
    InvalidAlignment,
    /// The requested alignment exceeds the maximum (4 MiB).
    #[display("Alignment exceeds maximum of 4 MiB")]
    AlignmentTooLarge,
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

    /// Serialization lock for tests that share `MOCK_PAGE_POOL`.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// RAII guard that serializes access to the global mock page pool.
    ///
    /// Acquiring a `TestPool` locks out other tests, installs a fresh
    /// pool backed by heap memory, and tears it down on drop.
    struct TestPool {
        _guard: std::sync::MutexGuard<'static, ()>,
        _backing: Vec<u8>,
    }

    impl TestPool {
        /// Create a zero-filled pool of `pages` pages.
        fn new(pages: usize) -> Self {
            Self::with_fill(pages, 0)
        }

        /// Create a pool of `pages` pages, each byte set to `fill`.
        fn with_fill(pages: usize, fill: u8) -> Self {
            let guard = TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let len = pages
                .checked_mul(PAGE_SIZE)
                .expect("TestPool::with_fill: pages * PAGE_SIZE overflow");
            let mut backing = vec![fill; len];
            let ptr = NonNull::new(backing.as_mut_ptr()).unwrap();
            *MOCK_PAGE_POOL.lock() = Some(MockPagePool {
                memory: ptr,
                size: backing.len(),
                watermark: 0,
            });
            Self {
                _guard: guard,
                _backing: backing,
            }
        }
    }

    impl Drop for TestPool {
        fn drop(&mut self) {
            *MOCK_PAGE_POOL.lock() = None;
        }
    }

    #[test]
    fn test_basic_allocation() {
        let _pool = TestPool::new(16);

        let alloc = unsafe {
            PageAllocation::new_uninit(PageAllocationConstraint::AnyAddress, None, 1, None)
        };
        assert!(alloc.is_ok());
        let alloc = alloc.unwrap();
        assert_eq!(alloc.pages(), 1);
    }

    #[test]
    fn test_zeroed_allocation() {
        let _pool = TestPool::with_fill(16, 0xff);

        let alloc = PageAllocation::new_zeroed(PageAllocationConstraint::AnyAddress, None, 1, None);
        assert!(alloc.is_ok());
        let alloc = alloc.unwrap();

        // Verify all bytes are zero
        assert!(alloc.as_u8_slice().iter().all(|&b| b == 0));
    }

    #[test]
    fn test_alignment() {
        let _pool = TestPool::new(64);

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
        let _pool = TestPool::new(1);

        // Try to allocate more than the pool size
        let alloc = unsafe {
            PageAllocation::new_uninit(PageAllocationConstraint::AnyAddress, None, 2, None)
        };
        assert_eq!(alloc.err(), Some(PageAllocationFailure::OutOfMemory));
    }

    #[test]
    fn test_invalid_alignment() {
        let _pool = TestPool::new(16);

        // Alignment must be power of two
        let alloc = unsafe {
            PageAllocation::new_uninit(PageAllocationConstraint::AnyAddress, None, 1, Some(3))
        };
        assert_eq!(alloc.err(), Some(PageAllocationFailure::InvalidAlignment));
    }

    #[test]
    fn test_alignment_too_large() {
        let _pool = TestPool::new(16);

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
        let _pool = TestPool::new(16);

        let alloc = unsafe {
            PageAllocation::new_uninit(PageAllocationConstraint::MaxAddress(0x1000), None, 1, None)
        };
        assert_eq!(
            alloc.err(),
            Some(PageAllocationFailure::UnsupportedConstraint)
        );
    }
}

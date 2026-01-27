// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Memory management interfaces.

use core::{mem::MaybeUninit, ptr::NonNull};

/// Constraints for page allocations.
/// Platforms need to provide support for at least `AnyAddress`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageAllocationConstraint<Address> {
    AnyAddress,
    MaxAddress(Address),
    FixedAddress(Address),
}

/// Interface trait for page allocations.
/// Note that implementers are expected to also implement `Drop` to free the allocated pages.
/// We cannot enforce this as a safety requirement because leaking memory is not unsafe,
/// but it is still required for correct operation.
pub trait PageAllocationIface<Address>: Sized {
    /// Selectable memory type for the allocation.
    /// Platforms not supporting multiple memory types can use `()`.
    type MemoryType;

    /// Error type for allocation failures.
    type Error;

    /// Smallest allocatable unit size in bytes.
    const PAGE_SIZE: usize;

    /// Allocates `pages` pages of memory using the platform page allocator.
    /// The memory is uninitialized.
    ///
    /// # Parameters
    /// - `alignment`: Optional alignment in bytes (must be a power of two).
    ///   If `None`, uses the platform's default alignment.
    ///
    /// # Safety
    /// The caller must ensure that the allocated memory is properly initialized before use.
    unsafe fn new_uninit(
        constraint: PageAllocationConstraint<Address>,
        memory_type: Self::MemoryType,
        pages: usize,
        alignment: Option<usize>,
    ) -> Result<Self, Self::Error>;

    /// Allocates `pages` pages of memory using the platform page allocator.
    /// The memory is zero-initialized.
    fn new_zeroed(
        constraint: PageAllocationConstraint<Address>,
        memory_type: Self::MemoryType,
        pages: usize,
        alignment: Option<usize>,
    ) -> Result<Self, Self::Error> {
        unsafe {
            // SAFETY: We immediately initialize the memory after allocation, before safe code can access it.
            let allocation = Self::new_uninit(constraint, memory_type, pages, alignment)?;
            let s: &mut [MaybeUninit<u8>] = core::slice::from_raw_parts_mut(
                allocation.as_ptr().cast(),
                pages * Self::PAGE_SIZE,
            );
            s.fill(MaybeUninit::new(0));
            Ok(allocation)
        }
    }

    /// Allocates `pages` pages of memory using the platform page allocator.
    /// The first init.len() bytes are initialized from the `init` slice, the rest is zero-initialized.
    fn new_init_prefix(
        constraint: PageAllocationConstraint<Address>,
        memory_type: Self::MemoryType,
        pages: usize,
        alignment: Option<usize>,
        init: &[u8],
    ) -> Result<Self, Self::Error> {
        assert!(pages * Self::PAGE_SIZE >= init.len());
        unsafe {
            // SAFETY: We immediately initialize the memory after allocation, before safe code can access it.
            let allocation = Self::new_uninit(constraint, memory_type, pages, alignment)?;
            let s: &mut [MaybeUninit<u8>] = core::slice::from_raw_parts_mut(
                allocation.as_ptr().cast(),
                pages * Self::PAGE_SIZE,
            );
            let (dinit, dzero) = s.split_at_mut(init.len());
            core::ptr::copy_nonoverlapping(init.as_ptr(), dinit.as_mut_ptr().cast(), init.len());
            dzero.fill(MaybeUninit::new(0));
            Ok(allocation)
        }
    }

    /// Returns the number of pages allocated.
    fn pages(&self) -> usize;

    /// Create a PageAllocation from a raw pointer and page count.
    /// Dropping the PageAllocation will free the pages.
    /// # Safety
    /// The caller must ensure that the pointer was allocated with
    /// `Self::new_*` and is valid for `pages` pages.
    /// Additionally the caller needs to ensure that the memory
    /// is properly initialized before use.
    unsafe fn from_raw(ptr: NonNull<u8>, pages: usize) -> Self;

    /// Consumes the PageAllocation and returns the raw pointer and page count.
    /// The caller is responsible for freeing the pages.
    fn into_raw(self) -> (NonNull<u8>, usize);

    /// Returns the raw pointer to the allocated memory.
    fn as_ptr(&self) -> *mut u8;

    /// Returns a slice to the allocated memory.
    fn as_u8_slice(&self) -> &[u8];

    /// Returns a mutable slice to the allocated memory.
    fn as_u8_slice_mut(&mut self) -> &mut [u8];
}

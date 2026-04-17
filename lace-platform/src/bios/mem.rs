// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! BIOS platform memory init.
//!
//! Reads INT 15h E820h into the shared [`crate::memmap::MEMORY_MAP`],
//! carves the Rust heap out of it, and hands the range to
//! `linked_list_allocator`. `PageAllocation` is provided by `memmap`.

use linked_list_allocator::LockedHeap;

use super::e820::get_memory_map;
use crate::e820::E820Entry;
use crate::mem::{MemAttributes, PageAllocationConstraint};
use crate::memmap::MEMORY_MAP;

pub const PAGE_SIZE: usize = 4096;

/// Heap size. Small by design: the Rust heap backs short-lived allocator
/// types (Vec, Box); large buffers go through the page allocator.
const HEAP_PAGES: usize = 4096; // 16 MiB

/// Upper bound on heap placement: BIOS only identity-maps the low 1 GiB.
const HEAP_MAX_ADDR: u64 = 0x4000_0000;

pub use crate::memmap::{MemoryType, PageAllocation};

#[global_allocator]
static GLOBAL_ALLOCATOR: LockedHeap = LockedHeap::empty();

pub(super) fn init() {
    let mut entries = [E820Entry::default(); 64];
    let count = get_memory_map(&mut entries);

    let heap_base = {
        let mut map = MEMORY_MAP.lock();
        map.add_e820_entries(entries[..count].iter().copied());
        map.allocate(
            PageAllocationConstraint::MaxAddress(HEAP_MAX_ADDR),
            MemoryType::LoaderData,
            HEAP_PAGES,
            None,
        )
        .expect("no room for Rust heap in memory map")
    };

    unsafe {
        GLOBAL_ALLOCATOR
            .lock()
            .init(heap_base as *mut u8, HEAP_PAGES * PAGE_SIZE);
    }
}

pub fn change_mem_attrs(
    _addr_range: core::ops::Range<u64>,
    _attrs: MemAttributes,
) -> Result<(), crate::Error> {
    Ok(())
}

pub fn nx_required() -> bool {
    false
}

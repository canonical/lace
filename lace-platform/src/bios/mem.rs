// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

use super::e820::{E820Entry, E820MemoryType, get_memory_map};
use crate::mem::MemAttributes;
use linked_list_allocator::LockedHeap;

#[global_allocator]
static GLOBAL_ALLOCATOR: LockedHeap = LockedHeap::empty();

pub(super) fn init() {
    let mut entries = [E820Entry::default(); 64];
    let count = get_memory_map(&mut entries);

    // We only use memory above 1MB (to avoid BIOS area and our own code loaded at 64K)
    // and below 1GiB (the only memory we have mapped).
    const LOW: u64 = 0x10_0000;
    const HIGH: u64 = 0x4000_0000;

    let mut best_start = 0;
    let mut best_size = 0;

    for entry in &entries[..count] {
        if E820MemoryType::from(entry.type_) != E820MemoryType::Usable {
            continue;
        }

        let start = entry.base.max(LOW);
        let end = (entry.base + entry.length).min(HIGH);

        if start >= end {
            continue;
        }

        let size = end - start;
        if size > best_size {
            best_start = start;
            best_size = size;
        }
    }

    if best_size > 0 {
        unsafe {
            GLOBAL_ALLOCATOR
                .lock()
                .init(best_start as *mut u8, best_size as usize);
        }
    } else {
        panic!("Could not find suitable memory for heap");
    }
}

pub const PAGE_SIZE: usize = 4096;

pub enum MemoryType {}

pub struct PageAllocation {}

pub fn change_mem_attrs(
    _addr_range: core::ops::Range<u64>,
    _attrs: MemAttributes,
) -> Result<(), crate::Error> {
    // No-op on BIOS.
    Ok(())
}

pub fn nx_required() -> bool {
    false
}

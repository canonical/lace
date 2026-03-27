// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! BIOS Global Allocator

use super::e820::{E820Entry, E820MemoryType, get_memory_map};
use linked_list_allocator::LockedHeap;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub fn init() {
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
            ALLOCATOR
                .lock()
                .init(best_start as *mut u8, best_size as usize);
        }
    } else {
        panic!("Could not find suitable memory for heap");
    }
}

// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Virt platform memory init.
//!
//! Streams `etc/e820` from fw_cfg into the shared
//! [`crate::memmap::MEMORY_MAP`], carves the Rust heap out of it, and
//! hands the range to `linked_list_allocator`. `PageAllocation` comes
//! from `memmap`.

#![cfg(target_arch = "x86_64")]

use core::mem::size_of;
use lace_drivers::fw_cfg::{FwCfg, FwCfgFile};
use linked_list_allocator::LockedHeap;
use zerocopy::{FromZeros, IntoBytes};

use crate::e820::E820Entry;
use crate::mem::{MemAttributes, PageAllocationConstraint};
use crate::memmap::MEMORY_MAP;

/// Rust heap size. Large buffers go through the page allocator.
const HEAP_PAGES: usize = 4096; // 16 MiB

pub const PAGE_SIZE: usize = 4096;

pub use crate::memmap::{MemoryType, PageAllocation};

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Stream `etc/e820` into the system memory map, one entry at a time,
/// via DMA continuation reads so we never need more than one entry of
/// scratch space.
pub(super) fn read_e820(fw_cfg: &FwCfg) {
    let file = fw_cfg
        .find_file(b"etc/e820")
        .expect("fw_cfg: etc/e820 missing");
    let n = file.size() as usize / size_of::<E820Entry>();
    let mut map = MEMORY_MAP.lock();
    for_each_e820(fw_cfg, &file, n, |e| {
        let E820Entry { base, length, type_ } = *e;
        map.add_region(base, length, type_.into());
    });
}

/// Allocate the Rust heap from the memory map and install it. All
/// firmware reservations should be applied before this call so they
/// constrain heap placement.
pub(super) fn init_heap() {
    let base = MEMORY_MAP
        .lock()
        .allocate(
            PageAllocationConstraint::AnyAddress,
            MemoryType::LoaderData,
            HEAP_PAGES,
            None,
        )
        .expect("no room for Rust heap in memory map");
    unsafe {
        ALLOCATOR
            .lock()
            .init(base as *mut u8, HEAP_PAGES * PAGE_SIZE);
    }
}

/// Stream `n` e820 entries from fw_cfg, calling `f` for each. First
/// entry is read via SELECT+READ (rewind); subsequent entries come from
/// DMA continuation reads.
fn for_each_e820(fw_cfg: &FwCfg, file: &FwCfgFile, n: usize, mut f: impl FnMut(&E820Entry)) {
    let mut entry = E820Entry::new_zeroed();
    if n > 0 {
        fw_cfg.read_file(file, entry.as_mut_bytes());
        f(&entry);
        for _ in 1..n {
            fw_cfg.read_continuation(entry.as_mut_bytes());
            f(&entry);
        }
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

// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! Physical memory map and page allocator.
//!
//! The memory map is both the record of firmware-visible memory and the
//! free list that backs page allocation. Allocations re-type a range away
//! from `Usable`; freeing re-types it back. Reservations are expressed
//! as `allocate(FixedAddress(addr), type_, pages, ...)`.
//!
//! Storage is a fixed-size inline array of [`MAX_REGIONS`] entries, so
//! the map needs no allocator to work. That lets the platform build the
//! map and reserve the firmware footprint before the Rust heap is up,
//! and carve the heap itself out of the map. We may later grow by
//! allocating a larger buffer from the map itself (memblock-style), but
//! 256 entries already covers realistic firmware memory maps.

use core::mem::MaybeUninit;
use core::ops::Range;
use core::ptr::NonNull;
use lace_util::Display;
use spin::Mutex;

use crate::e820::{E820Entry, E820MemoryType};
use crate::mem::{PageAllocationConstraint, PageAllocationIface};

/// Page size used by the allocator. The whole API operates in page units.
pub const PAGE_SIZE: u64 = 4096;

/// Maximum number of regions the inline storage holds. Firmware memory
/// maps in practice stay well below this after coalescing.
pub const MAX_REGIONS: usize = 256;

/// Memory region type.
///
/// Mirrors the e820 vocabulary with one extra variant: `LoaderData` for
/// firmware-internal allocations. `LoaderData` exports to the OS as
/// e820 `Usable` once firmware has handed off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryType {
    Usable,
    LoaderData,
    Reserved,
    AcpiReclaim,
    AcpiNvs,
    BadMemory,
    Unknown(u32),
}

impl From<E820MemoryType> for MemoryType {
    fn from(t: E820MemoryType) -> Self {
        match t {
            E820MemoryType::Usable => MemoryType::Usable,
            E820MemoryType::Reserved => MemoryType::Reserved,
            E820MemoryType::AcpiReclaim => MemoryType::AcpiReclaim,
            E820MemoryType::AcpiNvs => MemoryType::AcpiNvs,
            E820MemoryType::BadMemory => MemoryType::BadMemory,
        }
    }
}

impl From<u32> for MemoryType {
    /// Build a `MemoryType` from a raw e820 type code. Unknown codes are
    /// preserved verbatim so they round-trip through the map.
    fn from(raw: u32) -> Self {
        E820MemoryType::try_from(raw)
            .map(MemoryType::from)
            .unwrap_or(MemoryType::Unknown(raw))
    }
}

impl MemoryType {
    /// Convert to an e820 wire-format type code. `LoaderData` collapses to
    /// `Usable` (1) because firmware-internal scratch is free memory from
    /// the OS's perspective.
    pub fn to_e820(self) -> u32 {
        match self {
            MemoryType::Usable | MemoryType::LoaderData => 1,
            MemoryType::Reserved => 2,
            MemoryType::AcpiReclaim => 3,
            MemoryType::AcpiNvs => 4,
            MemoryType::BadMemory => 5,
            MemoryType::Unknown(v) => v,
        }
    }
}

/// A region in the physical memory map.
#[derive(Debug, Clone, Copy)]
pub struct MemoryRegion {
    pub base: u64,
    pub length: u64,
    pub type_: MemoryType,
}

/// Page allocation failure.
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq)]
pub enum PageAllocationError {
    #[display("Out of memory")]
    OutOfMemory,
    #[display("Alignment must be a page-multiple power of two")]
    InvalidAlignment,
    #[display("Requested fixed address range is not Usable")]
    FixedAddressNotAvailable,
}

/// Physical memory map and page allocator.
///
/// Regions are kept sorted by base address and, after each mutation,
/// adjacent regions of identical type are coalesced.
pub struct MemoryMap {
    regions: [MaybeUninit<MemoryRegion>; MAX_REGIONS],
    len: usize,
}

impl Default for MemoryMap {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryMap {
    pub const fn new() -> Self {
        Self {
            regions: [MaybeUninit::uninit(); MAX_REGIONS],
            len: 0,
        }
    }

    /// View of all regions, sorted by base address.
    pub fn regions(&self) -> &[MemoryRegion] {
        // SAFETY: indices 0..len are initialized by every method that
        // mutates the map.
        unsafe {
            core::slice::from_raw_parts(self.regions.as_ptr().cast::<MemoryRegion>(), self.len)
        }
    }

    fn regions_mut(&mut self) -> &mut [MemoryRegion] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self.regions.as_mut_ptr().cast::<MemoryRegion>(),
                self.len,
            )
        }
    }

    /// Insert a region. Callers must ensure regions do not overlap, as is
    /// the case for well-formed e820 tables.
    pub fn add_region(&mut self, base: u64, length: u64, type_: MemoryType) {
        self.push(MemoryRegion {
            base,
            length,
            type_,
        });
        self.regions_mut().sort_by_key(|r| r.base);
        self.coalesce();
    }

    /// Populate from an iterator of raw e820 entries.
    #[allow(dead_code)] // used by BIOS; virt streams entries one by one
    pub fn add_e820_entries(&mut self, entries: impl Iterator<Item = E820Entry>) {
        for e in entries {
            self.add_region(e.base, e.length, e.type_.into());
        }
    }

    /// End of the highest RAM-backed region below `limit`. Counts every
    /// type that represents real RAM (free, firmware heap, ACPI data),
    /// not just `Usable`; otherwise carving `AcpiNvs` at the top of RAM
    /// would make this return an end *below* the carved range and callers
    /// (MMIO window placement) would collide with it.
    #[allow(dead_code)] // used by virt to place PCI MMIO above RAM
    pub fn ram_end_below(&self, limit: u64) -> u64 {
        let mut end = 0u64;
        for r in self.regions() {
            let is_ram = matches!(
                r.type_,
                MemoryType::Usable
                    | MemoryType::LoaderData
                    | MemoryType::AcpiReclaim
                    | MemoryType::AcpiNvs
            );
            if is_ram && r.base < limit {
                let r_end = (r.base + r.length).min(limit);
                if r_end > end {
                    end = r_end;
                }
            }
        }
        end
    }

    /// Allocate `pages` pages, retyping the chosen range from `Usable` to
    /// `type_`.
    ///
    /// `alignment` must be a power of two and at least `PAGE_SIZE`. If
    /// omitted, defaults to `PAGE_SIZE`.
    ///
    /// With `FixedAddress(addr)`, the entire requested range must currently
    /// be `Usable`; otherwise the allocation fails.
    pub fn allocate(
        &mut self,
        constraint: PageAllocationConstraint<u64>,
        type_: MemoryType,
        pages: usize,
        alignment: Option<u64>,
    ) -> Result<u64, PageAllocationError> {
        let align = alignment.unwrap_or(PAGE_SIZE);
        if !align.is_power_of_two() || align < PAGE_SIZE {
            return Err(PageAllocationError::InvalidAlignment);
        }
        let size = (pages as u64)
            .checked_mul(PAGE_SIZE)
            .ok_or(PageAllocationError::OutOfMemory)?;
        if size == 0 {
            return Err(PageAllocationError::OutOfMemory);
        }

        let base = match constraint {
            PageAllocationConstraint::AnyAddress => self.find_free(size, align, u64::MAX)?,
            PageAllocationConstraint::MaxAddress(max) => self.find_free(size, align, max)?,
            PageAllocationConstraint::FixedAddress(addr) => {
                if addr % align != 0 {
                    return Err(PageAllocationError::InvalidAlignment);
                }
                self.check_fixed(addr, size)?;
                addr
            }
        };

        self.carve(base, size, type_);
        Ok(base)
    }

    /// Return `pages` pages starting at `base` to `Usable`.
    pub fn free(&mut self, base: u64, pages: usize) {
        let size = (pages as u64) * PAGE_SIZE;
        self.carve(base, size, MemoryType::Usable);
    }

    /// Scan for a free range top-down, returning the highest suitable
    /// base address across all Usable regions. Keeps low memory available
    /// for callers that actually need it (real-mode trampolines, legacy
    /// DMA, sub-4 GB ACPI tables).
    fn find_free(
        &self,
        size: u64,
        align: u64,
        max_end: u64,
    ) -> Result<u64, PageAllocationError> {
        let mut best: Option<u64> = None;
        for r in self.regions() {
            if r.type_ != MemoryType::Usable {
                continue;
            }
            let r_end = r.base.saturating_add(r.length).min(max_end);
            if r_end < r.base.saturating_add(size) {
                continue;
            }
            let a_start = (r_end - size) & !(align - 1);
            if a_start < r.base {
                continue;
            }
            if best.is_none_or(|b| a_start > b) {
                best = Some(a_start);
            }
        }
        best.ok_or(PageAllocationError::OutOfMemory)
    }

    fn check_fixed(&self, addr: u64, size: u64) -> Result<(), PageAllocationError> {
        let end = addr
            .checked_add(size)
            .ok_or(PageAllocationError::OutOfMemory)?;
        // After coalesce() any containing Usable span is a single region.
        for r in self.regions() {
            let r_end = r.base + r.length;
            if r.base <= addr && r_end >= end {
                return if r.type_ == MemoryType::Usable {
                    Ok(())
                } else {
                    Err(PageAllocationError::FixedAddressNotAvailable)
                };
            }
        }
        Err(PageAllocationError::FixedAddressNotAvailable)
    }

    /// Split regions so that [base, base+size) becomes a single region of
    /// `type_`, preserving the types of any surrounding slices. Operates
    /// in place on the inline storage.
    fn carve(&mut self, base: u64, size: u64, type_: MemoryType) {
        let end = base + size;

        // Find the span of regions that overlap or are enclosed by
        // [base, end). Sorted order lets us do this by bracketing.
        let regions = self.regions();
        let first = regions
            .iter()
            .position(|r| r.base + r.length > base)
            .unwrap_or(regions.len());
        let last = regions.iter().rposition(|r| r.base < end);

        // Build up to three replacement pieces: leading slice from the
        // first overlap, the new [base, end) region, and a trailing
        // slice from the last overlap.
        let mut pieces: [MemoryRegion; 3] = [MemoryRegion {
            base: 0,
            length: 0,
            type_: MemoryType::Usable,
        }; 3];
        let mut np = 0;

        if let Some(last) = last
            && first < regions.len()
        {
            let r_first = regions[first];
            if r_first.base < base {
                pieces[np] = MemoryRegion {
                    base: r_first.base,
                    length: base - r_first.base,
                    type_: r_first.type_,
                };
                np += 1;
            }
            pieces[np] = MemoryRegion {
                base,
                length: size,
                type_,
            };
            np += 1;
            let r_last = regions[last];
            let r_last_end = r_last.base + r_last.length;
            if r_last_end > end {
                pieces[np] = MemoryRegion {
                    base: end,
                    length: r_last_end - end,
                    type_: r_last.type_,
                };
                np += 1;
            }
        } else {
            // [base, end) doesn't touch any existing region: just insert.
            pieces[np] = MemoryRegion {
                base,
                length: size,
                type_,
            };
            np += 1;
        }

        let replace = first..last.map_or(first, |l| l + 1);
        self.splice(replace, &pieces[..np]);
        self.coalesce();
    }

    /// Replace `regions[range]` with `new`. Shifts the tail as needed.
    fn splice(&mut self, range: Range<usize>, new: &[MemoryRegion]) {
        let old_len = range.end - range.start;
        let tail_start = range.end;
        let tail_len = self.len - tail_start;
        let new_len = new.len();

        let final_len = self.len - old_len + new_len;
        assert!(
            final_len <= MAX_REGIONS,
            "MemoryMap exceeded MAX_REGIONS ({MAX_REGIONS})"
        );

        // Shift tail to make/remove room.
        let new_tail_start = range.start + new_len;
        if new_tail_start != tail_start {
            // SAFETY: all indices touched live in 0..MAX_REGIONS and the
            // source range is within initialized storage.
            unsafe {
                core::ptr::copy(
                    self.regions.as_ptr().add(tail_start),
                    self.regions.as_mut_ptr().add(new_tail_start),
                    tail_len,
                );
            }
        }

        // Write the new pieces.
        for (i, piece) in new.iter().enumerate() {
            self.regions[range.start + i].write(*piece);
        }

        self.len = final_len;
    }

    fn push(&mut self, r: MemoryRegion) {
        assert!(
            self.len < MAX_REGIONS,
            "MemoryMap exceeded MAX_REGIONS ({MAX_REGIONS})"
        );
        self.regions[self.len].write(r);
        self.len += 1;
    }

    /// Merge runs of adjacent regions with identical type.
    fn coalesce(&mut self) {
        let mut w = 0;
        for r in 0..self.len {
            // SAFETY: indices 0..self.len are initialized.
            let current = unsafe { self.regions[r].assume_init() };
            if w > 0 {
                // SAFETY: same invariant.
                let prev = unsafe { self.regions[w - 1].assume_init_mut() };
                if prev.type_ == current.type_ && prev.base + prev.length == current.base {
                    prev.length += current.length;
                    continue;
                }
            }
            self.regions[w].write(current);
            w += 1;
        }
        self.len = w;
    }

    /// Write an e820-equivalent view of the map to `out`, coalescing
    /// adjacent same-type regions and folding `LoaderData` into `Usable`.
    /// Returns the number of entries written. Silently truncates if
    /// `out` is too short.
    pub fn write_e820(&self, out: &mut [E820Entry]) -> usize {
        let mut n = 0;
        for r in self.regions() {
            let ty = r.type_.to_e820();
            if n > 0 {
                let prev = &mut out[n - 1];
                let prev_type = { prev.type_ };
                let prev_base = { prev.base };
                let prev_len = { prev.length };
                if prev_type == ty && prev_base + prev_len == r.base {
                    prev.length = prev_len + r.length;
                    continue;
                }
            }
            if n >= out.len() {
                break;
            }
            out[n] = E820Entry {
                base: r.base,
                length: r.length,
                type_: ty,
            };
            n += 1;
        }
        n
    }
}

/// System memory map, shared by all platforms that use this module
/// (bios and virt). Lives in BSS; no allocator required.
pub static MEMORY_MAP: Mutex<MemoryMap> = Mutex::new(MemoryMap::new());

/// Invoke `f` with exclusive access to the system memory map.
pub fn with_memory_map<R>(f: impl FnOnce(&mut MemoryMap) -> R) -> R {
    f(&mut MEMORY_MAP.lock())
}

/// Resource holder for a page allocation backed by the shared [`MEMORY_MAP`].
/// Platforms that use this module (bios, virt) re-export this as their
/// `PageAllocation` so it satisfies [`PageAllocationIface`].
pub struct PageAllocation {
    base: u64,
    pages: usize,
}

impl PageAllocationIface<u64> for PageAllocation {
    const PAGE_SIZE: usize = PAGE_SIZE as usize;

    type MemoryType = MemoryType;

    type Error = PageAllocationError;

    unsafe fn new_uninit(
        constraint: PageAllocationConstraint<u64>,
        memory_type: Option<MemoryType>,
        pages: usize,
        alignment: Option<usize>,
    ) -> Result<Self, PageAllocationError> {
        let ty = memory_type.unwrap_or(MemoryType::LoaderData);
        let align = alignment.map(|a| a as u64);
        let base = MEMORY_MAP.lock().allocate(constraint, ty, pages, align)?;
        Ok(PageAllocation { base, pages })
    }

    fn pages(&self) -> usize {
        self.pages
    }

    unsafe fn from_raw(ptr: NonNull<u8>, pages: usize) -> Self {
        PageAllocation {
            base: ptr.as_ptr() as u64,
            pages,
        }
    }

    fn into_raw(self) -> (NonNull<u8>, usize) {
        let (base, pages) = (self.base, self.pages);
        core::mem::forget(self);
        (NonNull::new(base as *mut u8).unwrap(), pages)
    }

    fn as_ptr(&self) -> *mut u8 {
        self.base as *mut u8
    }

    fn as_u8_slice(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self.as_ptr(), self.pages * PAGE_SIZE as usize)
        }
    }

    fn as_u8_slice_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self.as_ptr(), self.pages * PAGE_SIZE as usize)
        }
    }
}

impl Drop for PageAllocation {
    fn drop(&mut self) {
        MEMORY_MAP.lock().free(self.base, self.pages);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_map() -> MemoryMap {
        let mut m = MemoryMap::new();
        m.add_region(0, 0x10_0000, MemoryType::Reserved);
        m.add_region(0x10_0000, 0x8000_0000 - 0x10_0000, MemoryType::Usable);
        m.add_region(0x8000_0000, 0x8000_0000, MemoryType::Reserved);
        m
    }

    #[test]
    fn allocate_any_address_is_top_down() {
        let mut m = mk_map();
        let base = m
            .allocate(PageAllocationConstraint::AnyAddress, MemoryType::LoaderData, 4, None)
            .unwrap();
        assert_eq!(base, 0x8000_0000 - 4 * PAGE_SIZE);
        let r = m.regions().iter().find(|r| r.base == base).unwrap();
        assert_eq!(r.length, 4 * PAGE_SIZE);
        assert_eq!(r.type_, MemoryType::LoaderData);
    }

    #[test]
    fn allocate_alignment() {
        let mut m = mk_map();
        let base = m
            .allocate(
                PageAllocationConstraint::AnyAddress,
                MemoryType::LoaderData,
                1,
                Some(1 << 20),
            )
            .unwrap();
        assert_eq!(base % (1 << 20), 0);
    }

    #[test]
    fn allocate_max_address() {
        let mut m = mk_map();
        let base = m
            .allocate(
                PageAllocationConstraint::MaxAddress(0x20_0000),
                MemoryType::LoaderData,
                1,
                None,
            )
            .unwrap();
        assert!(base + PAGE_SIZE <= 0x20_0000);
    }

    #[test]
    fn allocate_fixed_reserves() {
        let mut m = mk_map();
        let facs = 0x2_0000u64;
        let err = m
            .allocate(
                PageAllocationConstraint::FixedAddress(facs),
                MemoryType::AcpiNvs,
                1,
                None,
            )
            .unwrap_err();
        assert_eq!(err, PageAllocationError::FixedAddressNotAvailable);

        let pg = 0x20_0000u64;
        let base = m
            .allocate(
                PageAllocationConstraint::FixedAddress(pg),
                MemoryType::AcpiNvs,
                1,
                None,
            )
            .unwrap();
        assert_eq!(base, pg);
        let r = m.regions().iter().find(|r| r.base == pg).unwrap();
        assert_eq!(r.type_, MemoryType::AcpiNvs);
        assert_eq!(r.length, PAGE_SIZE);
    }

    #[test]
    fn free_returns_to_usable_and_coalesces() {
        let mut m = mk_map();
        let initial = m.regions().len();
        let base = m
            .allocate(PageAllocationConstraint::AnyAddress, MemoryType::LoaderData, 2, None)
            .unwrap();
        m.free(base, 2);
        assert_eq!(m.regions().len(), initial);
        assert!(m.regions().iter().all(|r| r.type_ != MemoryType::LoaderData));
    }

    #[test]
    fn write_e820_collapses_loader_data() {
        let mut m = mk_map();
        m.allocate(PageAllocationConstraint::AnyAddress, MemoryType::LoaderData, 1, None)
            .unwrap();
        let mut out = [E820Entry::default(); 8];
        let n = m.write_e820(&mut out);
        assert_eq!(n, 3);
        assert_eq!({ out[0].type_ }, 2);
        assert_eq!({ out[1].type_ }, 1);
        assert_eq!({ out[2].type_ }, 2);
    }

    #[test]
    fn invalid_alignment_rejected() {
        let mut m = mk_map();
        let err = m
            .allocate(
                PageAllocationConstraint::AnyAddress,
                MemoryType::LoaderData,
                1,
                Some(3000),
            )
            .unwrap_err();
        assert_eq!(err, PageAllocationError::InvalidAlignment);
    }

    #[test]
    fn out_of_memory() {
        let mut m = MemoryMap::new();
        m.add_region(0x10_0000, 0x1000, MemoryType::Usable);
        let err = m
            .allocate(PageAllocationConstraint::AnyAddress, MemoryType::LoaderData, 2, None)
            .unwrap_err();
        assert_eq!(err, PageAllocationError::OutOfMemory);
    }
}

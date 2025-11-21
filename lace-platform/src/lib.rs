// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

use core::alloc::GlobalAlloc;

#[allow(dead_code)]
trait Platform: GlobalAlloc {
    type Error;
    type PhysicalAddress;

    fn get_memory_map();

    fn allocate_pages(number_of_pages: usize) -> *mut u8;

    fn free_pages(ptr: *mut u8);

    fn physical_address_to_pointer(address: Self::PhysicalAddress) -> *mut u8;

    fn match_dtb(dtb: &[u8]) -> Result<bool, Self::Error>;

    fn boot_linux(cfg: BootLinuxConfig) -> Result<(), Self::Error>;
}

#[allow(dead_code)]
struct BootLinuxConfig<'kernel, 'initrd, 'dtb, 'cmdline> {
    kernel: &'kernel [u8],
    initrd: Option<&'initrd [u8]>,
    dtb: Option<&'dtb [u8]>,
    cmdline: &'cmdline [u8],
}

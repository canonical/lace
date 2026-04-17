// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! Shared x86-64 Linux boot protocol implementation
//!
//! Parses the Linux setup header, loads the protected-mode kernel,
//! initrd, and command line, then jumps to the 64-bit kernel entry.
//! Platform-specific data (e820, ACPI RSDP, screen info) is provided
//! by the caller via [`LinuxBootInfo`].

use alloc::boxed::Box;
use alloc::vec;
use core::mem::size_of;

use super::linux_bootparam::{BootParams, SetupHeader};
use crate::e820::E820Entry;
use crate::mem::{self, PageAllocationConstraint, PageAllocationIface};

/// Platform-specific boot information provided by the caller.
pub struct LinuxBootInfo<'a> {
    /// E820 memory map entries.
    pub e820: &'a [E820Entry],
    /// Physical address of the ACPI RSDP (0 if not available).
    pub acpi_rsdp_addr: u64,
    /// Screen info for VGA console (None to leave zeroed / no VGA).
    pub screen_info: Option<ScreenInfoConfig>,
}

/// VGA console configuration.
pub struct ScreenInfoConfig {
    pub orig_video_mode: u8,
    pub orig_video_cols: u8,
    pub orig_video_lines: u8,
    pub orig_video_is_vga: u8,
    pub orig_video_points: u16,
}

/// Boot a Linux kernel using the x86-64 boot protocol.
///
/// This function does not return on success.
pub fn boot_linux(
    kernel: &[u8],
    initrd: Option<&[u8]>,
    cmdline: Option<&str>,
    info: &LinuxBootInfo,
) -> Result<(), &'static str> {
    // Parse Setup Header
    if kernel.len() < 0x1F1 + size_of::<SetupHeader>() {
        return Err("kernel too small");
    }

    let header_ptr = unsafe { kernel.as_ptr().add(0x1F1) as *const SetupHeader };
    let header = unsafe { *header_ptr };

    if header.header != 0x53726448 {
        return Err("bad setup header magic");
    }

    if header.boot_flag != 0xAA55 {
        return Err("bad boot flag");
    }

    // Allocate Boot Params (leaked — kernel takes ownership)
    let params = Box::leak(Box::new(BootParams::default()));
    params.hdr = header;

    // Load protected mode kernel
    let setup_sects = if header.setup_sects == 0 {
        4
    } else {
        header.setup_sects
    };
    let setup_size = (setup_sects as usize + 1) * 512;

    if kernel.len() < setup_size {
        return Err("kernel too small for setup sectors");
    }

    let protected_mode_kernel = &kernel[setup_size..];

    let alignment = if header.relocatable_kernel != 0 {
        header.kernel_alignment as usize
    } else {
        0
    };

    let kernel_len = protected_mode_kernel.len();
    let init_size = header.init_size as usize;
    let alloc_size = core::cmp::max(kernel_len, init_size);
    let alignment = alignment.max(mem::PAGE_SIZE);

    // Reserve enough memory for the kernel's init phase, but only copy
    // the file bytes in. The Linux boot protocol does not require the
    // init_size tail to be pre-zeroed; the kernel initialises it itself.
    let kernel_alloc = unsafe {
        mem::PageAllocation::new_uninit(
            PageAllocationConstraint::AnyAddress,
            None,
            mem::page_count(alloc_size),
            Some(alignment),
        )
    }
    .map_err(|_| "kernel allocation failed")?;
    let kernel_entry = kernel_alloc.as_ptr() as usize;
    unsafe {
        core::ptr::copy_nonoverlapping(
            protected_mode_kernel.as_ptr(),
            kernel_entry as *mut u8,
            kernel_len,
        );
    }
    // Kernel takes ownership of this memory.
    core::mem::forget(kernel_alloc);

    // Load initrd. PageAllocation guarantees the alignment we request.
    if let Some(initrd_data) = initrd {
        let initrd_alloc = unsafe {
            mem::PageAllocation::new_uninit(
                PageAllocationConstraint::AnyAddress,
                None,
                mem::page_count(initrd_data.len()),
                None,
            )
        }
        .map_err(|_| "initrd allocation failed")?;
        let initrd_ptr = initrd_alloc.as_ptr();
        unsafe {
            core::ptr::copy_nonoverlapping(initrd_data.as_ptr(), initrd_ptr, initrd_data.len());
        }
        params.hdr.ramdisk_image = initrd_ptr as u32;
        params.hdr.ramdisk_size = initrd_data.len() as u32;
        core::mem::forget(initrd_alloc);
    }

    // Load command line
    if let Some(cmdline_str) = cmdline {
        if cmdline_str.ends_with('\0') {
            params.hdr.cmd_line_ptr = cmdline_str.as_ptr() as u32;
        } else {
            let mut cmdline_buffer = vec![0u8; cmdline_str.len() + 1];
            cmdline_buffer[..cmdline_str.len()].copy_from_slice(cmdline_str.as_bytes());
            params.hdr.cmd_line_ptr = cmdline_buffer.as_ptr() as u32;
            core::mem::forget(cmdline_buffer);
        }
    }

    // Fill boot params
    params.hdr.type_of_loader = 0xFF;
    params.hdr.loadflags &= !(1 << 5); // Clear QUIET flag

    // Screen info (platform-specific)
    if let Some(ref si) = info.screen_info {
        params.screen_info.orig_video_mode = si.orig_video_mode;
        params.screen_info.orig_video_cols = si.orig_video_cols;
        params.screen_info.orig_video_lines = si.orig_video_lines;
        params.screen_info.orig_video_is_vga = si.orig_video_is_vga;
        params.screen_info.orig_video_points = si.orig_video_points;
    }

    // ACPI RSDP
    params.acpi_rsdp_addr = info.acpi_rsdp_addr;

    // E820 memory map
    let count = info.e820.len().min(128);
    params.e820_entries = count as u8;
    params.e820_table[..count].copy_from_slice(&info.e820[..count]);

    // Check 64-bit entry support
    if (header.xloadflags & 1) == 0 {
        return Err("kernel does not support 64-bit entry");
    }

    // Jump to kernel
    log::debug!("Jumping to Linux kernel at {:#x}", kernel_entry);
    let entry_64 = kernel_entry as u64 + 0x200;
    unsafe {
        core::arch::asm!(
            "cli",
            "jmp {entry}",
            entry = in(reg) entry_64,
            in("rsi") params,
            options(noreturn)
        );
    }
}

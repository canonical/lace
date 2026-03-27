// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! BIOS Linux boot support.

use crate::amd64::linux_bootparam::{BootE820Entry, BootParams, SetupHeader};
use crate::bios::e820;
use alloc::boxed::Box;
use alloc::vec;
use core::mem::size_of;

#[derive(Debug)]
pub enum BootLinuxError {
    LoadError,
    Unsupported,
    IoError,
}

impl core::fmt::Display for BootLinuxError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BootLinuxError::LoadError => write!(f, "Load error"),
            BootLinuxError::Unsupported => write!(f, "Unsupported"),
            BootLinuxError::IoError => write!(f, "IO Error"),
        }
    }
}

pub fn boot_linux(
    kernel: &[u8],
    initrd: Option<&[u8]>,
    cmdline: Option<&str>,
) -> Result<(), BootLinuxError> {
    // Parse Setup Header
    if kernel.len() < 0x1F1 + size_of::<SetupHeader>() {
        return Err(BootLinuxError::LoadError);
    }

    let header_ptr = unsafe { kernel.as_ptr().add(0x1F1) as *const SetupHeader };
    let header = unsafe { *header_ptr };

    if header.header != 0x53726448 {
        // "HdrS"
        return Err(BootLinuxError::LoadError);
    }

    if header.boot_flag != 0xAA55 {
        return Err(BootLinuxError::LoadError);
    }

    // Allocate Boot Params
    let params = Box::leak(Box::new(BootParams::default()));
    // Copy header
    params.hdr = header;

    // Load Kernel (Protected Mode Code)
    let setup_sects = if header.setup_sects == 0 {
        4
    } else {
        header.setup_sects
    };
    let setup_size = (setup_sects as usize + 1) * 512;

    if kernel.len() < setup_size {
        return Err(BootLinuxError::LoadError);
    }

    let protected_mode_kernel = &kernel[setup_size..];

    // Calculate alignment requirements
    let alignment = if header.relocatable_kernel != 0 {
        header.kernel_alignment as usize
    } else {
        0
    };

    // Calculate total size needed for kernel + init_size
    // We need to ensure that we reserve enough space for the kernel's initialization
    // which requires `init_size` bytes starting from the kernel load address.
    let kernel_len = protected_mode_kernel.len();
    let init_size = header.init_size as usize;
    let alloc_size = core::cmp::max(kernel_len, init_size);

    // Allocate buffer with extra space for alignment
    let mut kernel_buffer = vec![0u8; alloc_size + alignment];
    let buffer_addr = kernel_buffer.as_ptr() as usize;

    // Calculate aligned entry point
    let kernel_entry = if alignment > 0 {
        (buffer_addr + alignment - 1) & !(alignment - 1)
    } else {
        buffer_addr
    };

    // Copy kernel to aligned position
    let offset = kernel_entry - buffer_addr;
    kernel_buffer[offset..offset + kernel_len].copy_from_slice(protected_mode_kernel);

    core::mem::forget(kernel_buffer); // Leak

    // Load Initrd
    if let Some(initrd_data) = initrd {
        // Use the existing initrd data directly
        let initrd_addr = initrd_data.as_ptr() as u32;
        params.hdr.ramdisk_image = initrd_addr;
        params.hdr.ramdisk_size = initrd_data.len() as u32;

        crate::println!("Kernel Entry: 0x{:08X}", kernel_entry);
        crate::println!("Initrd Addr:  0x{:08X}", initrd_addr);
        crate::println!("Initrd Size:  0x{:08X}", initrd_data.len());
        crate::println!("Init Size:    0x{:08X}", init_size);

        let kernel_start = kernel_entry as u32;
        let kernel_end = kernel_start + init_size as u32;
        let initrd_start = initrd_addr;
        let initrd_end = initrd_start + initrd_data.len() as u32;

        if kernel_start < initrd_end && initrd_start < kernel_end {
            crate::println!("WARNING: Initrd overlaps with kernel initialization memory!");
        }

        let max_addr = header.initrd_addr_max;
        if initrd_addr > max_addr {
            crate::println!(
                "WARNING: Initrd above initrd_addr_max (0x{:08X})!",
                max_addr
            );
        }
    }

    // Load Cmdline
    if let Some(cmdline_str) = cmdline {
        // We need a null-terminated string.
        // If the input string is already null-terminated, we can use it directly.
        // Otherwise, we must allocate a new buffer.
        if cmdline_str.ends_with('\0') {
            params.hdr.cmd_line_ptr = cmdline_str.as_ptr() as u32;
        } else {
            let mut cmdline_buffer = vec![0u8; cmdline_str.len() + 1];
            cmdline_buffer[..cmdline_str.len()].copy_from_slice(cmdline_str.as_bytes());
            cmdline_buffer[cmdline_str.len()] = 0;

            params.hdr.cmd_line_ptr = cmdline_buffer.as_ptr() as u32;
            core::mem::forget(cmdline_buffer); // Leak
        }
    }

    // Fill Boot Params
    params.hdr.type_of_loader = 0xFF; // Undefined/Custom
    params.hdr.loadflags &= !(1 << 5); // Clear QUIET flag

    // Set up VGA console
    params.screen_info.orig_video_mode = 3;
    params.screen_info.orig_video_cols = 80;
    params.screen_info.orig_video_lines = 25;
    params.screen_info.orig_video_is_vga = 1;
    params.screen_info.orig_video_points = 16;

    // Get Memory Map
    let mut entries = [e820::E820Entry::default(); 128];
    let num_entries = e820::get_memory_map(&mut entries);

    // Write kernel memory map
    params.e820_entries = num_entries as u8;
    for (i, entry) in entries[..num_entries].iter().enumerate() {
        if i >= 128 {
            break;
        }
        params.e820_table[i] = BootE820Entry {
            addr: entry.base,
            size: entry.length,
            type_: entry.type_,
        };
    }

    // Check if 64-bit entry is supported
    // XLF_KERNEL_64 = 1
    if (header.xloadflags & 1) == 0 {
        crate::println!("Kernel does not support 64-bit entry (XLF_KERNEL_64 not set)");
        return Err(BootLinuxError::Unsupported);
    }

    // Jump to Kernel
    crate::println!("Jumping to Linux kernel at 0x{:08X}", kernel_entry);
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

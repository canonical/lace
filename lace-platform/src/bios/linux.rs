// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! BIOS Linux boot support.

use crate::amd64::linux::{LinuxBootInfo, ScreenInfoConfig, boot_linux as boot_linux_impl};
use crate::amd64::linux_bootparam::E820_MAX_ENTRIES_ZEROPAGE;
use crate::e820::E820Entry;
use lace_util::Display;

#[derive(Debug, Display)]
pub enum BootLinuxError {
    #[display("Load error: {}")]
    Load(&'static str),
}

pub fn boot_linux(
    kernel: &[u8],
    initrd: Option<&[u8]>,
    cmdline: Option<&str>,
) -> Result<(), BootLinuxError> {
    let mut e820 = [E820Entry::default(); E820_MAX_ENTRIES_ZEROPAGE];
    let n = crate::memmap::with_memory_map(|m| m.write_e820(&mut e820));
    let info = LinuxBootInfo {
        e820: &e820[..n],
        acpi_rsdp_addr: 0,
        screen_info: Some(ScreenInfoConfig {
            orig_video_mode: 3,
            orig_video_cols: 80,
            orig_video_lines: 25,
            orig_video_is_vga: 1,
            orig_video_points: 16,
        }),
    };
    boot_linux_impl(kernel, initrd, cmdline, &info).map_err(BootLinuxError::Load)
}

// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Julian Andres Klode <julian.klode@canonical.com>
//! Speedboot - Fast boot menu.
//!
//! This application scans all available disks for GRUB configuration files,
//! extracts menu entries, displays them to the user, and boots the selected entry.

#![cfg_attr(not(feature = "mock"), no_std)]
#![cfg_attr(not(feature = "mock"), no_main)]

mod bootflows;
mod text;

extern crate alloc;

use alloc::string::String;

#[lace_platform::entry]
fn main() -> Result<(), lace_platform::Error> {
    lace_platform::println!("Lace Speedboot - Fast Boot Menu");
    lace_platform::println!("================================\n");

    // Discover boot configurations from all boot flows
    let mut boot_configs =
        bootflows::discover_all().expect("Failed to discover boot configurations");

    // Display menu
    text::display_menu(&boot_configs);

    // Get user selection
    let selection =
        text::get_user_selection(boot_configs.len()).expect("Failed to get user selection");

    // Boot the selected entry
    boot_configs
        .remove(selection)
        .start()
        .expect("Failed to boot the selected entry");

    Ok(())
}

/// Errors that can occur during speedboot operation.
#[derive(Debug)]
pub enum SpeedbootError {
    PlatformError(lace_platform::Error),
    NoBootEntriesFound,
    InvalidSelection,
    NoKernelPath,
    FileNotFound,
    FileReadError,
    BootError(String),
}

impl core::fmt::Display for SpeedbootError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SpeedbootError::PlatformError(e) => write!(f, "Platform error: {}", e),
            SpeedbootError::NoBootEntriesFound => write!(f, "No boot entries found"),
            SpeedbootError::InvalidSelection => write!(f, "Invalid selection"),
            SpeedbootError::NoKernelPath => write!(f, "No kernel path in menu entry"),
            SpeedbootError::FileNotFound => write!(f, "Boot file not found"),
            SpeedbootError::FileReadError => write!(f, "Failed to read boot file"),
            SpeedbootError::BootError(msg) => write!(f, "Boot error: {}", msg),
        }
    }
}

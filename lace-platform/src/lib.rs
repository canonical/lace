// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Platform abstractions for Lace.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use lace_util::fdt::node::FdtNode;

// Interfaces for platform implementations.
// These are not strictly necessary as the active platform is chosen at compile time,
// but they help to clarify the expected API surface for each platform.
pub mod iface;

// Platform implementations
#[cfg(feature = "efi")]
pub mod efi;
#[cfg(feature = "mock")]
pub mod mock;

#[cfg(feature = "efi")]
use efi as p;
#[cfg(feature = "mock")]
use mock as p;

// Re-export portable APIs from the active platform at the top-level namespace.
// The list of APIs exported here constitutes the portable Lace platform API.
pub use p::Error;

/// The macros below are implemented using these.
pub use p::{print_impl, println_impl};

// Macros for text output that should always be available
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::print_impl(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {
        $crate::println_impl(format_args!($($arg)*))
    };
}

// Macros for debug text output that is only active in debug builds
#[macro_export]
#[cfg(debug_assertions)]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::print_impl(format_args!($($arg)*))
    };
}

#[macro_export]
#[cfg(not(debug_assertions))]
macro_rules! debug {
    ($($arg:tt)*) => {};
}

#[macro_export]
#[cfg(debug_assertions)]
macro_rules! debugln {
    ($($arg:tt)*) => {
        $crate::println_impl(format_args!($($arg)*))
    };
}

#[macro_export]
#[cfg(not(debug_assertions))]
macro_rules! debugln {
    ($($arg:tt)*) => {};
}

// Re-export derive macros
pub use lace_util_derive::entry;

pub use p::find_edid;
pub use p::find_smbios_tables;

pub mod dtb {
    pub use super::p::dtb::{find_dtb, install_dtb};
}

pub mod linux {
    pub use super::p::linux::{BootLinuxError, boot_linux};
}

/// Determine platform compatibility using firmware-provided device tree.
/// # Safety
/// This function is unsafe because the compatible string references the firmware DTB,
/// which may be invalidated by other code replacing it.
pub unsafe fn platform_compatible_using_firmware_dtb() -> Option<&'static str> {
    let dtb = unsafe { dtb::find_dtb() }?;
    dtb.find_node("/")
        .and_then(FdtNode::compatible)
        .and_then(|compatible| compatible.all().next())
}

/// Determine platform compatibility using CHID mappings and sources.
pub fn platform_compatible_using_hwids(hwids: &[u8]) -> Option<&str> {
    // Parse CHID mappings from hwids section
    let mappings: Vec<lace_util::chid_mapping::ChidMapping> =
        lace_util::chid_mapping::ChidMappingIterator::from(hwids)
            .collect::<Result<_, _>>()
            .ok()?;
    debugln!("Parsed {} CHID mappings", mappings.len());

    // Get CHID sources
    let sources = chid_sources()?;
    for (idx, src) in sources.iter().enumerate() {
        debugln!("CHID Source {}: {:?}", idx, src);
    }

    // Compute CHIDs
    for (idx, chid_type) in lace_util::chid::CHID_TYPES.iter().enumerate() {
        debugln!(
            "CHID type {}: {:?}",
            idx,
            lace_util::chid::compute_chid(&sources, *chid_type)
        );
    }

    // Match mappings
    for mapping in lace_util::chid_matcher::ChidMatcher::new(&mappings, &sources) {
        if let lace_util::chid_mapping::ChidMapping::DeviceTree {
            compatible: Some(compatible),
            ..
        } = &mapping
        {
            return Some(compatible);
        }
    }
    None
}

/// Get CHID sources from SMBIOS and EDID tables provided by the platform.
pub fn chid_sources() -> Option<lace_util::chid::ChidSources> {
    let (ep, table) = find_smbios_tables()?;
    let edid = find_edid();
    lace_util::chid::chid_sources_from_smbios_and_edid(Some(ep), table, edid.as_deref()).ok()
}

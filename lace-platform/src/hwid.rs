// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! lace-platform hardware identification interfaces

use alloc::vec::Vec;
use lace_util::fdt::node::FdtNode;

/// Determine platform compatibility using firmware-provided device tree.
/// # Safety
/// This function is unsafe because the compatible string references the firmware DTB,
/// which may be invalidated by other code replacing it.
pub unsafe fn platform_compatible_using_firmware_dtb() -> Option<&'static str> {
    let dtb = unsafe { find_dtb() }?;
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
    log::debug!("Parsed {} CHID mappings", mappings.len());

    // Get CHID sources
    let sources = chid_sources()?;
    for (idx, src) in sources.iter().enumerate() {
        log::debug!("CHID Source {}: {:?}", idx, src);
    }

    // Compute CHIDs
    for (idx, chid_type) in lace_util::chid::CHID_TYPES.iter().enumerate() {
        log::debug!(
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

// Re-export platform specific implementations
pub use crate::p::hwid::{find_dtb, find_edid, find_smbios_tables, install_dtb};

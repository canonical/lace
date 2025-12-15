// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! UEFI EDID Discovered Protocol.

/// This protocol contains the EDID information retrieved from a video output device.
/// Ref: UEFI specification, 12.9.2.4. EFI_EDID_DISCOVERED_PROTOCOL
#[repr(C)]
pub struct EdidDiscoveredProtocol {
    pub size_of_edid: u32,
    pub edid: *const u8,
}

unsafe impl uefi::Identify for EdidDiscoveredProtocol {
    const GUID: uefi::Guid = uefi::guid!("1c0c34f6-d380-41fa-a049-8ad06c1a66aa");
}

impl uefi::proto::Protocol for EdidDiscoveredProtocol {}

impl EdidDiscoveredProtocol {
    /// Get the EDID data as a byte slice, if available.
    pub fn edid_data(&self) -> &[u8] {
        if self.size_of_edid == 0 || self.edid.is_null() {
            &[]
        } else {
            unsafe {
                // SAFETY: edid is a valid pointer to size_of_edid bytes.
                // The slice will have the same lifetime as &self.
                core::slice::from_raw_parts(self.edid, self.size_of_edid as usize)
            }
        }
    }
}

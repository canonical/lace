// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! TPM2 interface module.

use bitflags::bitflags;

bitflags! {
    /// Flags for the "hash_log_extend_event" operation.
    pub struct ExtendFlags: u32 {
        /// Only extend the PCR, do not log the event.
        /// Best effort, not required to be implemented on every platform.
        const EXTEND_ONLY = 0x0000_0001;
        /// Measure an Authenticode hash instead of flat hash of a PE/COFF image.
        /// Best effort, not required to be implemented on non-EFI platforms.
        const PE_COFF_IMAGE = 0x0000_0010;
    }
}

/// Event types for the "hash_log_extend_event" operation.
/// See 10.4.1 of the TCG PC Platform Firmware Profile Specification.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum EventType {
    // Generic event types
    PrebootCert = 0x0000_0000,
    PostCode = 0x0000_0001,
    Unused = 0x0000_0002,
    NoAction = 0x0000_0003,
    Separator = 0x0000_0004,
    Action = 0x0000_0005,
    EventTag = 0x0000_0006,
    SCRTMContent = 0x0000_0007,
    SCRTMVersion = 0x0000_0008,
    CPUMicrocode = 0x0000_0009,
    PlatformConfigFlags = 0x0000_000a,
    TableOfDevices = 0x0000_000b,
    CompactHash = 0x0000_000c,
    IPL = 0x0000_000d,
    IPLPartitionData = 0x0000_000e,
    NonHostCode = 0x0000_000f,
    NonHostConfig = 0x0000_0010,
    NonHostInfo = 0x0000_0011,
    OmitBootDeviceEvents = 0x0000_0012,
    PostCode2 = 0x0000_0013,

    // EFI-specific event types
    EFIEventBase = 0x8000_0000,
    EfiVariableDriverConfig = 0x8000_0001,
    EfiVariableBoot = 0x8000_0002,
    EfiBootServicesApplication = 0x8000_0003,
    EfiBootServicesDriver = 0x8000_0004,
    EfiRuntimeServicesDriver = 0x8000_0005,
    EfiGptEvent = 0x8000_0006,
    EfiAction = 0x8000_0007,
    EfiPlatformFirmwareBlob = 0x8000_0008,
    EfiHandoffTables = 0x8000_0009,
    EfiPlatformFirmwareBlob2 = 0x8000_000a,
    EfiHandoffTables2 = 0x8000_000b,
    EfiVariableBoot2 = 0x8000_000c,
    EfiGptEvent2 = 0x8000_000d,
    EfiHCRTMEvent = 0x8000_0010,
    EfiVariableAuthority = 0x8000_00e0,
    EfiSpdmFirmwareBlob = 0x8000_00e1,
    EfiSpdmFirmwareConfig = 0x8000_00e2,
    EfiSpdmDevicePolicy = 0x8000_00e3,
    EfiSpdmDeviceAuthority = 0x8000_00e4,
}

impl EventType {
    /// Returns the raw u32 value of the event type.
    pub fn raw(self) -> u32 {
        self as u32
    }
}

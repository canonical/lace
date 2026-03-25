// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! A simple UEFI application that installs a fake EDID protocol
//! with data read from edid.bin file located at the root of the
//! filesystem the application was booted from.

#![cfg_attr(target_os = "uefi", no_main)]
#![cfg_attr(target_os = "uefi", no_std)]

#[cfg(target_os = "uefi")]
mod fakeedid;

#[cfg(target_os = "uefi")]
#[uefi::entry]
fn main() -> uefi::Status {
    fakeedid::run()
}

#[cfg(not(target_os = "uefi"))]
fn main() {}

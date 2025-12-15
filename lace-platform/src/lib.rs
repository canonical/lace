// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Platform abstractions for Lace.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

// Platform implementations
// TODO(mkukri): choose which platform we build and alias as 'p'
// based on build target.
pub mod efi;
// pub mod ...;

use efi as p;
// use ... as p;

// Re-export portable APIs from the active platform at the top-level namespace.
// The list of APIs exported here constitutes the portable Lace platform API.
pub use p::Error;
pub use p::debugln;
pub use p::find_edid;
pub use p::find_smbios_tables;

pub mod dtb {
    pub use super::p::dtb::{find_dtb, install_dtb};
}

pub mod linux {
    pub use super::p::linux::{BootLinuxError, boot_linux};
}

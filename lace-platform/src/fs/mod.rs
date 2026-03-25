// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Julian Andres Klode <julian.klode@canonical.com>

//! Filesystem drivers and abstractions
pub mod base;
#[cfg(feature = "ext4")]
pub mod ext4;
pub mod gpt;
pub mod mbr;
pub mod probe;
#[cfg(test)]
pub(crate) mod testutil;

// Re-export common filesystem types from the base module
pub use base::*;

// Re-export probe API
pub use probe::{probe_all, probe_boot_device};

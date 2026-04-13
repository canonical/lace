// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! lace-platform Linux kernel loader interfaces

// Re-export platform specific implementations
pub use crate::p::linux::{BootLinuxError, boot_linux};

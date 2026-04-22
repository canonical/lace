// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Mock platform abstractions.

pub mod console;
pub mod fs;
pub mod hwid;
pub mod linux;
pub mod mem;
pub mod tpm2;

#[derive(Debug, lace_util::Display)]
#[display("mock platform error")]
pub struct Error;

impl std::error::Error for Error {}

/// Shared mock-side initialization, invoked from `#[lace_platform::entry]`.
pub fn init() {
    crate::console::init();
}

/// Mock has no persistent platform storage, so there's no
/// `speedboot.toml` to return.
pub fn speedboot_toml() -> Option<alloc::vec::Vec<u8>> {
    None
}

// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Mock placeholder for a Linux kernel loader.

#[derive(Debug, lace_util::Display)]
#[display("mock platform boot linux error")]
pub struct BootLinuxError;

pub fn boot_linux(
    _kernel: &[u8],
    _initrd: Option<&[u8]>,
    _cmdline: Option<&str>,
) -> Result<(), BootLinuxError> {
    Err(BootLinuxError)
}

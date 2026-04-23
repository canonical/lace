// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! Hardware drivers and device access for bare-metal environments

#![cfg_attr(not(test), no_std)]

extern crate alloc;

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod x86;

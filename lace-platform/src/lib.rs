// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Platform abstractions for Lace.

#![cfg_attr(not(feature = "mock"), no_std)]

extern crate alloc;

// Architecture-specific modules
#[cfg(target_arch = "x86_64")]
pub mod amd64;

// Platform implementations
#[cfg(feature = "bios")]
pub mod bios;
#[cfg(feature = "efi")]
pub mod efi;
#[cfg(feature = "mock")]
pub mod mock;
#[cfg(feature = "virt")]
pub mod virt;

#[cfg(feature = "bios")]
use bios as p;
#[cfg(feature = "efi")]
use efi as p;
#[cfg(feature = "mock")]
use mock as p;
#[cfg(feature = "virt")]
use virt as p;

// Re-export platform error type
pub use p::Error;

// Re-export entry point macro
pub use lace_util_derive::entry;

pub mod console;
pub mod fs;
pub mod hwid;
pub mod linux;
pub mod e820;
pub mod mem;
#[cfg(not(feature = "efi"))]
mod memmap;
pub mod tpm2;

// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Platform abstractions for Lace.
//!
//! The active platform is chosen from the build target's `target_os`:
//! `uefi` → EFI, `bios` → legacy BIOS, `virt` → QEMU virt firmware,
//! anything else → the hosted mock platform used for tests.

#![cfg_attr(not(unix), no_std)]

extern crate alloc;

// Architecture-specific modules
#[cfg(target_arch = "x86_64")]
pub mod amd64;

// Platform implementations
#[cfg(target_os = "bios")]
pub mod bios;
#[cfg(target_os = "uefi")]
pub mod efi;
#[cfg(unix)]
pub mod mock;
#[cfg(target_os = "virt")]
pub mod virt;

#[cfg(target_os = "bios")]
use bios as p;
#[cfg(target_os = "uefi")]
use efi as p;
#[cfg(unix)]
use mock as p;
#[cfg(target_os = "virt")]
use virt as p;

// Re-export platform error type
pub use p::Error;

// Re-export entry point macro
pub use lace_util_derive::entry;

pub mod console;
pub mod e820;
pub mod fs;
pub mod hwid;
pub mod linux;
pub mod mem;
#[cfg(any(target_os = "bios", target_os = "virt"))]
mod memmap;
pub mod tpm2;

// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Platform abstractions for Lace.

#![cfg_attr(not(feature = "mock"), no_std)]

extern crate alloc;

// Portable platform interface modules. Each defines the cross-platform
// types and re-exports the active platform's concrete implementation.
pub mod mem;
pub mod tpm2;

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

#[cfg(feature = "bios")]
use bios as p;
#[cfg(feature = "efi")]
use efi as p;
#[cfg(feature = "mock")]
use mock as p;

// Re-export portable APIs from the active platform at the top-level namespace.
// The list of APIs exported here constitutes the portable Lace platform API.
pub use p::Error;

// Macros for text output that should always be available
#[macro_export]
macro_rules! print {
    () => {};
    ($($arg:tt)*) => {
        $crate::console::print_impl(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! println {
    () => {
        $crate::console::println_impl(format_args!(""))
    };
    ($($arg:tt)*) => {
        $crate::console::println_impl(format_args!($($arg)*))
    };
}

// Macros for debug text output that is only active in debug builds
#[macro_export]
#[cfg(debug_assertions)]
macro_rules! debug {
    () => {};
    ($($arg:tt)*) => {
        $crate::console::print_impl(format_args!($($arg)*))
    };
}

#[macro_export]
#[cfg(not(debug_assertions))]
macro_rules! debug {
    ($($arg:tt)*) => {};
}

#[macro_export]
#[cfg(debug_assertions)]
macro_rules! debugln {
    () => {
        $crate::console::println_impl(format_args!(""))
    };
    ($($arg:tt)*) => {
        $crate::console::println_impl(format_args!($($arg)*))
    };
}

#[macro_export]
#[cfg(not(debug_assertions))]
macro_rules! debugln {
    ($($arg:tt)*) => {};
}

pub use p::console;

// Re-export derive macros
pub use lace_util_derive::entry;

pub mod hwid;

// Unified filesystem module re-exporting common types and platform-specific functions
pub mod fs;

pub mod linux {
    pub use super::p::linux::{BootLinuxError, boot_linux};
}

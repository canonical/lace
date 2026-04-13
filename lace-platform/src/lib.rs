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

#[cfg(feature = "bios")]
use bios as p;
#[cfg(feature = "efi")]
use efi as p;
#[cfg(feature = "mock")]
use mock as p;

// Re-export platform error type
pub use p::Error;

// Re-export entry point macro
pub use lace_util_derive::entry;

// Macros for text output that should always be available
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        use core::fmt::Write as _;
        write!($crate::console::stdout(), $($arg)*).unwrap()
    }};
}

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {{
        use core::fmt::Write as _;
        writeln!($crate::console::stdout(), $($arg)*).unwrap()
    }};
}

// Macros for debug text output that is only active in debug builds
#[macro_export]
#[cfg(debug_assertions)]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::print!($($arg)*)
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
    ($($arg:tt)*) => {
        $crate::println!($($arg)*)
    };
}

#[macro_export]
#[cfg(not(debug_assertions))]
macro_rules! debugln {
    ($($arg:tt)*) => {};
}

pub mod console;
pub mod fs;
pub mod hwid;
pub mod linux;
pub mod mem;
pub mod tpm2;

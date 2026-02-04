// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Sandbox platform abstractions.

extern crate std;

use core::{fmt, ops::Deref};

pub mod mem;

#[derive(Debug)]
pub struct Error;

pub fn print_impl(args: fmt::Arguments<'_>) {
    std::print!("{}", args);
}

pub fn println_impl(args: fmt::Arguments<'_>) {
    std::println!("{}", args);
}

struct EdidRef;

impl Deref for EdidRef {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        todo!()
    }
}

pub fn find_edid() -> Option<impl Deref<Target = [u8]>> {
    Option::<EdidRef>::None
}

pub fn find_smbios_tables() -> Option<(&'static [u8], &'static [u8])> {
    todo!()
}

pub mod dtb {
    use lace_util::fdt::Fdt;

    /// Finds an installed DTB in the system.
    /// # Safety
    /// This is not implemented for now.
    pub unsafe fn find_dtb() -> Option<Fdt<'static>> {
        todo!()
    }

    /// Installs a DTB in the system.
    /// # Safety
    /// This is not implemented for now.
    pub unsafe fn install_dtb(_dtb: &[u8]) -> Result<(), super::Error> {
        todo!()
    }
}

pub mod linux {
    #[derive(Debug)]
    pub struct BootLinuxError;

    pub fn boot_linux(
        _kernel: &[u8],
        _initrd: Option<&[u8]>,
        _cmdline: Option<&str>,
    ) -> Result<(), BootLinuxError> {
        todo!()
    }
}

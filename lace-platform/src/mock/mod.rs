// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Sandbox platform abstractions.

pub mod console;
pub mod mem;
pub mod tpm2;

use lace_util::Display;

#[derive(Debug, Display)]
#[display("mock platform error")]
pub struct Error;

impl std::error::Error for Error {}

struct EdidRef;

impl std::ops::Deref for EdidRef {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        todo!()
    }
}

pub fn find_edid() -> Option<impl std::ops::Deref<Target = [u8]>> {
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

    /// Placeholder for a DTB installation receipt.
    pub struct MockDtbReceipt;

    /// Installs a DTB in the system.
    /// # Safety
    /// This is not implemented for now.
    pub unsafe fn install_dtb(_dtb: &[u8]) -> Result<MockDtbReceipt, super::Error> {
        todo!()
    }
}

pub mod fs;

pub mod linux {
    use lace_util::Display;

    #[derive(Debug, Display)]
    #[display("mock platform boot linux error")]
    pub struct BootLinuxError;

    pub fn boot_linux(
        _kernel: &[u8],
        _initrd: Option<&[u8]>,
        _cmdline: Option<&str>,
    ) -> Result<(), BootLinuxError> {
        todo!()
    }
}

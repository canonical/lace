// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! UEFI platform abstractions.

pub mod console;
pub mod fs;
pub mod hwid;
pub mod image;
pub mod linux;
pub mod mem;
pub mod proto;
pub mod tpm2;

/// Platform specific error type
pub use uefi::Error;

/// Re-export common filesystem types from the base module
pub use super::fs::base::{BlockDevice, DirEntry, File, Filesystem, FsError};

/// Opens the first instance of the given protocol in exclusive mode.
fn open_first_protocol_exclusive<T: uefi::proto::Protocol>()
-> Result<uefi::boot::ScopedProtocol<T>, uefi::Error> {
    let handle_buf =
        uefi::boot::locate_handle_buffer(uefi::boot::SearchType::ByProtocol(&T::GUID))?;
    // SAFETY: locate_handle_buffer() returns EFI_NOT_FOUND if no handles are found.
    uefi::boot::open_protocol_exclusive::<T>(handle_buf[0])
}

/// EFI main function, initializes global state and calls the application entry point.
#[uefi::entry]
fn efi_main() -> uefi::Status {
    uefi::helpers::init().unwrap();
    mem::init();

    unsafe extern "Rust" {
        fn lace_app_main() -> Result<(), Error>;
    }

    match unsafe { lace_app_main() } {
        Ok(()) => uefi::Status::SUCCESS,
        Err(e) => e.status(),
    }
}

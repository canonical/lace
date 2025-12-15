// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! UEFI platform abstractions.

pub mod mem;

/// Platform specific error type
pub use uefi::Error;

/// Platform specific debug output
pub use uefi::println as debugln;

/// Opens the first instance of the given protocol in exclusive mode.
pub fn open_protocol_exclusive<T: uefi::proto::Protocol>()
-> Result<uefi::boot::ScopedProtocol<T>, uefi::Error> {
    let handle_buf =
        uefi::boot::locate_handle_buffer(uefi::boot::SearchType::ByProtocol(&T::GUID))?;
    // SAFETY: locate_handle_buffer() returns EFI_NOT_FOUND if no handles are found.
    uefi::boot::open_protocol_exclusive::<T>(handle_buf[0])
}

/// Finds a configuration table by its GUID.
pub fn find_config_table<T>(guid: uefi::Guid) -> Option<*const T> {
    uefi::system::with_config_table(|tables| {
        for table in tables.iter() {
            if table.guid.eq(&guid) && !table.address.is_null() {
                return Some(table.address as *const T);
            }
        }
        None
    })
}

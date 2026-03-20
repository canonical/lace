// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! UEFI platform abstractions.

use core::ops::Deref;
use lace_util::smbios::{Smbios3EntryPoint, SmbiosEntryPoint};

pub mod console;
pub mod dtb;
pub mod image;
pub mod linux;
pub mod mem;
pub mod proto;
pub mod tpm2;

use proto::edid_discovered::EdidDiscoveredProtocol;

/// Platform specific error type
pub use uefi::Error;

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

/// Opaque reference to EDID data obtained from the EDID Discovered Protocol.
struct EdidRef(uefi::boot::ScopedProtocol<EdidDiscoveredProtocol>);

impl Deref for EdidRef {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.0.edid_data()
    }
}

/// Finds the first EDID Discovered Protocol instance and returns the EDID data attached to it.
pub fn find_edid() -> Option<impl Deref<Target = [u8]>> {
    open_protocol_exclusive::<EdidDiscoveredProtocol>()
        .ok()
        .map(EdidRef)
}

/// Finds SMBIOS tables in the UEFI configuration tables.
pub fn find_smbios_tables() -> Option<(&'static [u8], &'static [u8])> {
    unsafe {
        if let Some(ptr) =
            find_config_table::<Smbios3EntryPoint>(uefi::table::cfg::ConfigTableEntry::SMBIOS3_GUID)
            && (*ptr).anchor_string.eq(b"_SM3_")
        {
            return Some((
                core::slice::from_raw_parts(ptr as *const u8, size_of::<Smbios3EntryPoint>()),
                core::slice::from_raw_parts(
                    (*ptr).table_address as *const u8,
                    (*ptr).table_maximum_size as _,
                ),
            ));
        }
        if let Some(ptr) =
            find_config_table::<SmbiosEntryPoint>(uefi::table::cfg::ConfigTableEntry::SMBIOS_GUID)
            && (*ptr).anchor_string.eq(b"_SM_")
        {
            return Some((
                core::slice::from_raw_parts(ptr as *const u8, size_of::<SmbiosEntryPoint>()),
                core::slice::from_raw_parts((*ptr).table_address as _, (*ptr).table_length as _),
            ));
        }
    }
    None
}

/// EFI main function, initializes global state and calls the application entry point.
#[uefi::entry]
fn efi_main() -> uefi::Status {
    uefi::helpers::init().unwrap();
    mem::efi_mem_init();

    unsafe extern "Rust" {
        fn lace_app_main() -> Result<(), Error>;
    }

    match unsafe { lace_app_main() } {
        Ok(()) => uefi::Status::SUCCESS,
        Err(e) => e.status(),
    }
}

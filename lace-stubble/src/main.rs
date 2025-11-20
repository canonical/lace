// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
// UEFI main application

#![no_main]
#![no_std]
#![cfg(not(test))]

use lace_util::peimage;
use lace_util::smbios::*;
use uefi::Guid;
use uefi::boot;
use uefi::prelude::*;
use uefi::proto::loaded_image::LoadedImage;
use uefi::system;
use uefi::table::cfg::ConfigTableEntry;

#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();
    uefi::println!("UEFI main()");

    // Parse SMBIOS
    let s: &'static _ = find_smbios_tables().unwrap();
    uefi::println!("SMBIOS table {:#?} {}", s.as_ptr(), s.len());
    system::with_stdout(|w| lace_util::hexdump(w, s)).unwrap();

    let smbios0 = find_smbios_table_by_type::<SmbiosTableType0>(s, 0).expect("need table 0");
    uefi::println!("{:#x?}", smbios0.table());
    let smbios1 = find_smbios_table_by_type::<SmbiosTableType1>(s, 1).expect("need table 1");
    uefi::println!("{:#x?}", smbios1.table());

    let not_found = find_smbios_table_by_type::<SmbiosHeader>(s, 42);
    assert!(matches!(not_found, Err(SmbiosError::TableNotFound)));

    // Parse our own loaded image
    let li = boot::open_protocol_exclusive::<LoadedImage>(boot::image_handle())
        .expect("cannot get our own loaded image");
    let (ptr, len) = li.info();
    let li_slice = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let pe = peimage::parse_pe(li_slice).expect("failed to parse our own PE image");

    uefi::println!("{:#x?}", pe.nt_hdrs);
    uefi::println!("{:#x?}", pe.sect_hdrs);

    uefi::println!("PE sections");
    for sect in pe.sect_hdrs.iter() {
        uefi::println!("{:#x?}", str::from_utf8(sect.name()).unwrap());
    }

    loop {}
}

fn find_smbios_tables() -> Option<&'static [u8]> {
    unsafe {
        if let Some(ptr) = find_config_table::<Smbios3EntryPoint>(ConfigTableEntry::SMBIOS3_GUID) {
            if (*ptr).anchor_string.eq(b"_SM3_") {
                return Some(core::slice::from_raw_parts(
                    (*ptr).table_address as _,
                    (*ptr).table_maximum_size as _,
                ));
            }
        }
        if let Some(ptr) = find_config_table::<SmbiosEntryPoint>(ConfigTableEntry::SMBIOS_GUID) {
            if (*ptr).anchor_string.eq(b"_SM_") {
                return Some(core::slice::from_raw_parts(
                    (*ptr).table_address as _,
                    (*ptr).table_length as _,
                ));
            }
        }
    }
    None
}

fn find_config_table<T>(guid: Guid) -> Option<*const T> {
    uefi::system::with_config_table(|tables| {
        for table in tables.iter() {
            if table.guid.eq(&guid) && !table.address.is_null() {
                return Some(table.address as *const T);
            }
        }
        None
    })
}

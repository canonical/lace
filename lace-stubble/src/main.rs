// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
// UEFI main application

#![no_main]
#![no_std]
#![cfg(not(test))]

use lace_util::smbios::Smbios3EntryPoint;
use lace_util::smbios::SmbiosEntryPoint;
use uefi::Guid;
use uefi::prelude::*;
use uefi::table::cfg::ConfigTableEntry;

/*
#[panic_handler]
fn panic_handler(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
*/

#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();
    uefi::println!("UEFI main()");


    let s: &'static _ = find_smbios_tables().unwrap();
    uefi::println!("SMBIOS table {:#?} {}", s.as_ptr(), s.len());
    hexdump(s);

    loop {}
}

fn hexdump(s: &[u8]) {
    for (i, b) in s.iter().enumerate() {
        if i % 16 == 0 {
            uefi::print!("{:04x} ", i)
        }
        uefi::print!("{:02x}", b);
        if (i+1)%16 == 0 || i+1 == s.len() {
            uefi::println!()
        } else if (i+1)%8 == 0 {
            uefi::print!("  ")
        } else {
            uefi::print!(" ")
        }
    }
}

fn find_smbios_tables() -> Option<&'static [u8]> {
    unsafe {
        if let Some(ptr) = find_config_table::<Smbios3EntryPoint>(ConfigTableEntry::SMBIOS3_GUID) {
            if (*ptr).anchor_string.eq(b"_SM3_") {
                return Some(core::slice::from_raw_parts((*ptr).table_address as _, (*ptr).table_maximum_size as _));
            }
        }
        if let Some(ptr) = find_config_table::<SmbiosEntryPoint>(ConfigTableEntry::SMBIOS_GUID) {
            if (*ptr).anchor_string.eq(b"_SM_") {
                return Some(core::slice::from_raw_parts((*ptr).table_address as _, (*ptr).table_length as _));
            }
        }
    }
    None
}

fn find_config_table<T>(guid: Guid) -> Option<*const T> {
    uefi::system::with_config_table(|tables| {
        for table in tables.iter() {
            if table.guid.eq(&guid) && !table.address.is_null() {
                return Some(table.address as *const T)
            } 
        }
        None
    })
}

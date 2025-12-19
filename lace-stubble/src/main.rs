// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
// UEFI main application

#![no_main]
#![no_std]

extern crate alloc;

use alloc::string::String;

#[uefi::entry]
fn main() -> uefi::Status {
    uefi::helpers::init().unwrap();

    // Parse own loaded image
    let li = uefi::boot::open_protocol_exclusive::<uefi::proto::loaded_image::LoadedImage>(
        uefi::boot::image_handle(),
    )
    .expect("cannot get own loaded image");

    // Convert loaded image data to slice
    let li_slice = unsafe {
        // SAFETY: This is valid unless the firmware is seriously broken
        let (ptr, len) = li.info();
        core::slice::from_raw_parts(ptr as *const u8, len as usize)
    };

    // Get external cmdline if any
    let external_cmdline: Option<String> = match li.load_options_as_cstr16() {
        Ok(cstr16) => Some(
            core::char::decode_utf16(cstr16.to_u16_slice().iter().cloned())
                .map(|r| r.unwrap_or(core::char::REPLACEMENT_CHARACTER))
                .collect(),
        ),
        Err(uefi::proto::loaded_image::LoadOptionsError::NotSet) => None,
        Err(e) => {
            uefi::println!("Invalid load options: {:?}", e);
            None
        }
    };

    // Boot the stubble image
    lace_stubble::boot_stubble_image(li_slice, None, external_cmdline.as_deref())
        .expect("Failed to boot");

    unreachable!()
}

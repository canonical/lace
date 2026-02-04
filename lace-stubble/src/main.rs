// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
// UEFI main application

#![cfg_attr(target_os = "uefi", no_main)]
#![cfg_attr(target_os = "uefi", no_std)]

extern crate alloc;

use alloc::string::String;
use lace_platform::{Error, debugln};

#[cfg_attr(target_os = "uefi", lace_platform::entry)]
fn main() -> Result<(), Error> {
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
    let external_cmdline: Option<String> = match li.load_options_as_bytes() {
        Some(bytes) => {
            let s: String = core::char::decode_utf16(
                bytes
                    .chunks(2)
                    .map(|chunk| u16::from_ne_bytes([chunk[0], chunk[1]])),
            )
            .map(|r| {
                r.unwrap_or_else(|err| {
                    debugln!("WARNING: command line contains invalid character: {}", err);
                    core::char::REPLACEMENT_CHARACTER
                })
            })
            .collect();
            Some(s)
        }
        None => None,
    };

    // Boot the stubble image
    lace_stubble::boot_stubble_image(
        lace_stubble::StubbleImage::Loaded(li_slice),
        None,
        external_cmdline.as_deref(),
    )
    .expect("Failed to boot");

    unreachable!()
}

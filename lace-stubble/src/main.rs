// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
// UEFI main application

#![no_main]
#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use lace_platform::dtb::install_dtb;
use lace_platform::linux::boot_linux;
use lace_util::peimage::parse_pe;

#[uefi::entry]
fn main() -> uefi::Status {
    uefi::helpers::init().unwrap();

    // Parse own loaded image
    let li = uefi::boot::open_protocol_exclusive::<uefi::proto::loaded_image::LoadedImage>(
        uefi::boot::image_handle(),
    )
    .expect("cannot get own loaded image");
    let li_slice = unsafe {
        // SAFETY: This is valid unless the firmware is seriously broken
        let (ptr, len) = li.info();
        core::slice::from_raw_parts(ptr as *const u8, len as usize)
    };
    let pe = parse_pe(li_slice).expect("failed to parse own PE image");

    // Find relevant sections
    let mut kernel = None;
    let mut initrd = None;
    let mut cmdline = None;
    let mut hwids = None;
    let mut dtbauto: Vec<&[u8]> = Vec::new();

    uefi::println!("PE sections");
    for result in pe.virtual_sections() {
        let (sect, data) = result.expect("failed to read section");
        uefi::println!(
            "  {:<8} {:08x} {:08x}",
            str::from_utf8(sect.name()).unwrap(),
            sect.virtual_address,
            sect.virtual_size
        );

        match sect.name() {
            b".linux" => kernel = Some(data),
            b".initrd" => initrd = Some(data),
            b".cmdline" => cmdline = Some(data),
            b".hwids" => hwids = Some(data),
            b".dtbauto" => dtbauto.push(data),
            _ => {}
        }
    }

    // Ensure kernel is present
    let kernel = kernel.expect("cannot boot without .linux section");

    // First try to get platform compatible from firmware DTB
    // If that fails, try using CHID matching against .hwids section
    let compatible =
        unsafe { lace_platform::platform_compatible_using_firmware_dtb() }.or_else(|| {
            hwids
                .map(|hwids| lace_platform::platform_compatible_using_hwids(hwids))
                .unwrap_or(None)
        });
    uefi::println!(
        "Determined platform compatible: {}",
        compatible.unwrap_or("<none>")
    );

    // Find suitable DTB from .dtbauto sections
    // Parse using lace_util::fdt::Fdt then get compatible like below
    if let Some(compatible) = compatible {
        let mut installed = false;
        for dtb_data in dtbauto {
            let dtb_fdt = match lace_util::fdt::Fdt::new(dtb_data) {
                Ok(fdt) => fdt,
                Err(e) => {
                    uefi::println!("  skipping invalid .dtbauto section: {}", e);
                    continue;
                }
            };
            if dtb_fdt.root().compatible().first() == compatible {
                uefi::println!("Installing DTB for compatible {}", compatible);
                unsafe {
                    install_dtb(dtb_data).expect("failed to install DTB");
                }
                installed = true;
                break;
            }
        }
        if !installed {
            uefi::println!(
                "No matching DTB found for compatible {}, skipping DTB installation",
                compatible
            );
        }
    } else {
        uefi::println!("No platform compatible determined, skipping DTB installation");
    }

    // Boot the kernel
    boot_linux(kernel, initrd, cmdline).expect("failed to start linux");

    #[allow(clippy::empty_loop)]
    loop {}
}

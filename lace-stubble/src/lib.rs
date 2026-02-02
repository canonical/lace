// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Lace Stubble library

#![no_std]

extern crate alloc;

use alloc::{vec, vec::Vec};
use core::fmt::Display;
use lace_platform::debugln;
use lace_platform::dtb::install_dtb;
use lace_platform::linux::boot_linux;
use lace_util::peimage::{PeError, SectionHeader, parse_pe};
use uefi::proto::tcg::v2::PcrEventInputs;

/// Errors that can occur when booting a Stubble image.
#[derive(Clone, Copy, Debug)]
pub enum BootStubbleError {
    PeError(PeError),
    NotAStubbleImage,
    InvalidCommandLine,
}

impl Display for BootStubbleError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BootStubbleError::PeError(e) => write!(f, "PE parsing error: {}", e),
            BootStubbleError::NotAStubbleImage => write!(f, "Not a Stubble image"),
            BootStubbleError::InvalidCommandLine => write!(f, "Invalid command line encoding"),
        }
    }
}

/// A stubble image can be handled either already loaded or in raw form
pub enum StubbleImage<'a> {
    Loaded(&'a [u8]),
    Raw(&'a [u8]),
}

/// Boots a Stubble image with an optional initrd and command line.
/// The initrd and command line will only be used if the Stubble image does not
/// contain corresponding sections (.initrd and .cmdline).
pub fn boot_stubble_image<'image>(
    stubble_image: StubbleImage<'image>,
    initrd: Option<&[u8]>,
    cmdline: Option<&str>,
) -> Result<(), BootStubbleError> {
    // Parse image
    let (data, raw) = match stubble_image {
        StubbleImage::Loaded(s) => (s, false),
        StubbleImage::Raw(s) => (s, true),
    };
    let pe = parse_pe(data).map_err(BootStubbleError::PeError)?;

    // Parsed sections/data
    let mut kernel = None;
    let mut initrd = initrd;
    let mut cmdline = cmdline;
    let mut hwids = None;
    let mut dtbauto: Vec<&[u8]> = Vec::new();

    // Closure to process each section
    let section_filter =
        |result: Result<(SectionHeader, &'image [u8]), PeError>| -> Result<(), BootStubbleError> {
            let (sect, data) = result.map_err(BootStubbleError::PeError)?;
            debugln!(
                "  {:<8} {:08x} {:08x}",
                str::from_utf8(sect.name()).unwrap(),
                sect.virtual_address,
                sect.virtual_size
            );

            match sect.name() {
                b".linux" => kernel = Some(data),
                b".initrd" => initrd = Some(data),
                b".cmdline" => {
                    cmdline = Some(
                        core::str::from_utf8(data)
                            .map_err(|_| BootStubbleError::InvalidCommandLine)?,
                    )
                }
                b".hwids" => hwids = Some(data),
                b".dtbauto" => dtbauto.push(data),
                _ => {}
            }
            Ok(())
        };

    debugln!("PE sections");
    if raw {
        pe.raw_sections().try_for_each(section_filter)?;
    } else {
        pe.virtual_sections().try_for_each(section_filter)?;
    }

    // Ensure kernel is present
    let kernel = kernel.ok_or(BootStubbleError::NotAStubbleImage)?;

    // First try to get platform compatible from firmware DTB
    // If that fails, try using CHID matching against .hwids section
    let compatible =
        unsafe { lace_platform::platform_compatible_using_firmware_dtb() }.or_else(|| {
            hwids
                .map(|hwids| lace_platform::platform_compatible_using_hwids(hwids))
                .unwrap_or(None)
        });
    debugln!(
        "Determined platform compatible: {}",
        compatible.unwrap_or("<none>")
    );

    // Find suitable DTB from .dtbauto sections
    // Keep installed dtb receipt here so it is in scope for the kernel boot
    let mut installed_dtb = None;
    if let Some(compatible) = compatible {
        for dtb_data in dtbauto {
            let dtb_fdt = match lace_util::fdt::Fdt::new(dtb_data) {
                Ok(fdt) => fdt,
                Err(e) => {
                    debugln!("Skipping invalid .dtbauto section: {}", e);
                    continue;
                }
            };
            let Some(dtb_compatible) = dtb_fdt
                .find_node("/")
                .and_then(|n| n.compatible())
                .and_then(|compatible| compatible.all().next())
            else {
                debugln!("Skipping .dtbauto section with no compatible property");
                continue;
            };
            if dtb_compatible == compatible {
                debugln!("Installing DTB for compatible {}", compatible);
                installed_dtb =
                    unsafe { Some(install_dtb(dtb_data).expect("failed to install DTB")) };
                break;
            }
        }
        if installed_dtb.is_none() {
            debugln!(
                "No matching DTB found for compatible {}, skipping DTB installation",
                compatible
            );
        }
    } else {
        debugln!("No platform compatible determined, skipping DTB installation");
    }

    // Measure command line to TPM 2.0 - PCR 12
    use uefi::proto::tcg;
    if let Ok(mut tcg2) = lace_platform::efi::open_protocol_exclusive::<tcg::v2::Tcg>() {
        let cmdline = cmdline.unwrap_or_default();
        // Prepare buffer for event
        // Unfortunately the exact size of PcrEventInputs header is not exposed,
        // so we allocate a bit more than needed.
        let mut event_buf = vec![0u8; 64 + cmdline.len()];
        let _ = PcrEventInputs::new_in_buffer(
            &mut event_buf,
            // Kernel command line is measured into PCR 12,
            // see https://uapi-group.org/specifications/specs/linux_tpm_pcr_registry
            tcg::PcrIndex(12),
            tcg::EventType::IPL,
            cmdline.as_bytes(),
        )
        .map_err(|err| err.to_err_without_payload())
        .and_then(|event| {
            tcg2.hash_log_extend_event(
                tcg::v2::HashLogExtendEventFlags::empty(),
                cmdline.as_bytes(),
                event,
            )
        })
        .inspect_err(|err| {
            debugln!("Failed to measure kernel command line: {}", err);
        });
    } else {
        debugln!("TPM 2.0 TCG protocol not available, skipping command line measurement");
    }

    // Boot the kernel
    boot_linux(kernel, initrd, cmdline).expect("failed to start linux");

    unreachable!()
}

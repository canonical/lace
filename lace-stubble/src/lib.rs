// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Stubble library

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use lace_platform::hwid::install_dtb;
use lace_platform::linux::boot_linux;
use lace_platform::tpm2::{self, EventType, ExtendFlags};
use lace_util::Display;
use lace_util::peimage::{PeError, SectionHeader, parse_pe};

/// Errors that can occur when booting a Stubble image.
#[derive(Clone, Copy, Debug, Display)]
pub enum BootStubbleError {
    #[display("PE parsing error: {}")]
    PeError(PeError),
    #[display("Duplicate section in Stubble image: {}")]
    DuplicateSection(&'static str),
    #[display("Not a Stubble image")]
    NotAStubbleImage,
    #[display("Invalid command line encoding")]
    InvalidCommandLine,
}

/// A stubble image can be handled either already loaded or in raw form
pub enum StubbleImage<'a> {
    Loaded(&'a [u8]),
    Raw(&'a [u8]),
}

/// Boots a Stubble image with an optional initrd and command line.
/// The external_initrd and external_cmdline will only be used if the Stubble image does not
/// contain corresponding sections (.initrd and .cmdline).
pub fn boot_stubble_image<'image>(
    stubble_image: StubbleImage<'image>,
    external_initrd: Option<&[u8]>,
    external_cmdline: Option<&str>,
) -> Result<(), BootStubbleError> {
    // Parse image
    let (data, raw) = match stubble_image {
        StubbleImage::Loaded(s) => (s, false),
        StubbleImage::Raw(s) => (s, true),
    };
    let pe = parse_pe(data).map_err(BootStubbleError::PeError)?;

    // Parsed sections/data
    let mut kernel = None;
    let mut initrd = None;
    let mut cmdline = None;
    let mut hwids = None;
    let mut dtbauto: Vec<&[u8]> = Vec::new();

    // Closure to process each section
    let section_filter =
        |result: Result<(SectionHeader, &'image [u8]), PeError>| -> Result<(), BootStubbleError> {
            let (sect, data) = result.map_err(BootStubbleError::PeError)?;
            log::debug!(
                "  {:<8} {:08x} {:08x}",
                str::from_utf8(sect.name()).unwrap(),
                sect.virtual_address,
                sect.virtual_size
            );

            match sect.name() {
                b".linux" => kernel
                    .insert_once_or_error(data, BootStubbleError::DuplicateSection(".linux"))?,
                b".initrd" => initrd
                    .insert_once_or_error(data, BootStubbleError::DuplicateSection(".initrd"))?,
                b".cmdline" => {
                    let cmdline_str = core::str::from_utf8(data)
                        .map_err(|_| BootStubbleError::InvalidCommandLine)?;
                    cmdline.insert_once_or_error(
                        cmdline_str,
                        BootStubbleError::DuplicateSection(".cmdline"),
                    )?
                }
                b".hwids" => hwids
                    .insert_once_or_error(data, BootStubbleError::DuplicateSection(".hwids"))?,
                b".dtbauto" => dtbauto.push(data),
                _ => {}
            }
            Ok(())
        };

    log::debug!("PE sections");
    if raw {
        pe.raw_sections().try_for_each(section_filter)?;
    } else {
        pe.virtual_sections().try_for_each(section_filter)?;
    }

    // Use external initrd and/or cmdline if not present in image
    if let (Some(external_cmdline), true) = (external_cmdline, cmdline.is_none()) {
        cmdline = Some(external_cmdline);
    }
    if let (Some(external_initrd), true) = (external_initrd, initrd.is_none()) {
        initrd = Some(external_initrd);
    }

    // Ensure kernel is present
    let kernel = kernel.ok_or(BootStubbleError::NotAStubbleImage)?;

    // First try to get platform compatible from firmware DTB
    // If that fails, try using CHID matching against .hwids section
    let compatible = unsafe { lace_platform::hwid::platform_compatible_using_firmware_dtb() }
        .or_else(|| {
            hwids
                .map(|hwids| lace_platform::hwid::platform_compatible_using_hwids(hwids))
                .unwrap_or(None)
        });
    log::debug!(
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
                    log::debug!("Skipping invalid .dtbauto section: {}", e);
                    continue;
                }
            };
            let Some(dtb_compatible) = dtb_fdt
                .find_node("/")
                .and_then(|n| n.compatible())
                .and_then(|compatible| compatible.all().next())
            else {
                log::debug!("Skipping .dtbauto section with no compatible property");
                continue;
            };
            if dtb_compatible == compatible {
                log::debug!("Installing DTB for compatible {}", compatible);
                installed_dtb =
                    unsafe { Some(install_dtb(dtb_data).expect("failed to install DTB")) };
                break;
            }
        }
        if installed_dtb.is_none() {
            log::debug!(
                "No matching DTB found for compatible {}, skipping DTB installation",
                compatible
            );
        }
    } else {
        log::debug!("No platform compatible determined, skipping DTB installation");
    }

    // Measure kernel command line to TPM 2.0 - PCR 12
    // See https://uapi-group.org/specifications/specs/linux_tpm_pcr_registry
    let cmdline_or_default = cmdline.unwrap_or_default();
    match tpm2::hash_log_extend_event(
        12,
        ExtendFlags::empty(),
        EventType::IPL.raw(),
        cmdline_or_default.as_bytes(),
        cmdline_or_default.as_bytes(),
    ) {
        Ok(()) => (),
        Err(err) => {
            log::debug!("Failed to measure kernel command line: {}", err);
        }
    }

    // Boot the kernel
    boot_linux(kernel, initrd, cmdline).expect("failed to start linux");

    unreachable!()
}

/// Extension trait to insert a value into an Option only if it is None,
/// otherwise return an error.
trait InsertOnce<T, E> {
    /// Inserts the value if the Option is None, otherwise returns the provided error.
    fn insert_once_or_error(&mut self, value: T, err: E) -> Result<(), E>;
}

impl<T, E> InsertOnce<T, E> for Option<T> {
    fn insert_once_or_error(&mut self, value: T, err: E) -> Result<(), E> {
        if self.is_some() {
            Err(err)
        } else {
            *self = Some(value);
            Ok(())
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_insert_once_success() {
        let mut opt: Option<i32> = None;
        let result = opt.insert_once_or_error(42, "error");
        assert_eq!(result, Ok(()));
        assert_eq!(opt, Some(42));
    }

    #[test]
    fn test_insert_once_duplicate_error() {
        let mut opt: Option<i32> = Some(10);
        let result = opt.insert_once_or_error(42, "duplicate");
        assert_eq!(result, Err("duplicate"));
        assert_eq!(opt, Some(10)); // Original value should be preserved
    }

    #[test]
    fn test_insert_once_with_string() {
        let mut opt: Option<&str> = None;
        let result = opt.insert_once_or_error("hello", "error");
        assert_eq!(result, Ok(()));
        assert_eq!(opt, Some("hello"));
    }

    #[test]
    fn test_insert_once_with_slice() {
        let mut opt: Option<&[u8]> = None;
        let data: &[u8] = &[1, 2, 3];
        let result = opt.insert_once_or_error(data, "error");
        assert_eq!(result, Ok(()));
        assert_eq!(opt, Some(data));
    }
}

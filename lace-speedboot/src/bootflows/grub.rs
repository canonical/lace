// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Julian Andres Klode <julian.klode@canonical.com>
//! GRUB boot flow implementation.

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;
use lace_platform::fs::{Filesystem, FsError};
use lace_platform::println;
use lace_util::grub::{MenuEntry, parse_grub_cfg};

use super::{BootConfiguration, BootFlow, SimpleBootConfiguration};
use crate::SpeedbootError;

/// Common locations for grub.cfg files.
const GRUB_PATHS: &[&str] = &[
    "boot/grub/grub.cfg",
    "grub/grub.cfg",
    "boot/grub2/grub.cfg",
    "grub2/grub.cfg",
    "EFI/ubuntu/grub.cfg",
    "EFI/debian/grub.cfg",
    "EFI/fedora/grub.cfg",
];

/// Ensure a path starts with a leading slash.
fn ensure_leading_slash(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    }
}

/// GRUB boot flow implementation.
pub struct GrubBootFlow;

impl GrubBootFlow {
    /// Create a new GRUB boot flow.
    pub fn new() -> Self {
        Self
    }

    /// Build boot configurations by expanding submenus with prefixed titles.
    fn build_boot_configurations(
        &self,
        entries: &[MenuEntry],
        filesystem: Rc<RefCell<Box<dyn Filesystem>>>,
    ) -> Vec<Box<dyn BootConfiguration>> {
        let mut result = Vec::new();
        for entry in entries {
            Self::flatten_recursive(entry, "", Rc::clone(&filesystem), &mut result);
        }
        result
    }

    /// Recursively build boot configurations from menu entries.
    fn flatten_recursive(
        entry: &MenuEntry,
        prefix: &str,
        filesystem: Rc<RefCell<Box<dyn Filesystem>>>,
        result: &mut Vec<Box<dyn BootConfiguration>>,
    ) {
        match entry {
            MenuEntry::Entry {
                title,
                linux,
                initrd,
                cmdline,
            } => {
                let full_title = if prefix.is_empty() {
                    title.clone()
                } else {
                    alloc::format!("{} > {}", prefix, title)
                };
                // This is a bootable entry
                result.push(Box::new(SimpleBootConfiguration::new(
                    full_title,
                    linux.clone(),
                    initrd.clone(),
                    cmdline.clone(),
                    filesystem,
                )));
            }
            MenuEntry::Submenu {
                title,
                entries: subentries,
            } => {
                let full_title = if prefix.is_empty() {
                    title.clone()
                } else {
                    alloc::format!("{} > {}", prefix, title)
                };

                // Recursively flatten submenu
                for subentry in subentries {
                    Self::flatten_recursive(subentry, &full_title, Rc::clone(&filesystem), result);
                }
            }
        }
    }
}

impl BootFlow for GrubBootFlow {
    fn discover(
        &self,
        filesystem: Rc<RefCell<Box<dyn Filesystem>>>,
    ) -> Result<Vec<Box<dyn BootConfiguration>>, SpeedbootError> {
        let mut all_configs = Vec::new();

        // Try each known GRUB configuration path
        for grub_path in GRUB_PATHS {
            let path = ensure_leading_slash(grub_path);
            if let Ok(mut file) = filesystem.borrow_mut().open_file(&path) {
                println!("  Found: {}", grub_path);

                if let Ok(content_bytes) = file.read_to_end()
                    && let Ok(content) = core::str::from_utf8(&content_bytes)
                {
                    let entries = parse_grub_cfg(content);
                    println!("    Parsed {} menu entries", entries.len());
                    all_configs.append(
                        &mut self.build_boot_configurations(&entries, Rc::clone(&filesystem)),
                    );
                }
            }
        }

        if all_configs.is_empty() {
            return Err(SpeedbootError::NoBootEntriesFound);
        }

        Ok(all_configs)
    }
}

/// Try to open a file, checking both the original path and /boot/<path>.
fn try_open_boot_file(
    fs: &mut dyn Filesystem,
    path: &str,
) -> Result<Box<dyn lace_platform::fs::File>, FsError> {
    // Try original path first
    if let Ok(file) = fs.open_file(&ensure_leading_slash(path)) {
        return Ok(file);
    }

    // Try /boot/<path> if not already prefixed with /boot
    if !path.starts_with("boot/") && !path.starts_with("/boot/") {
        let boot_path = format!("/boot/{}", path.trim_start_matches('/'));
        if let Ok(file) = fs.open_file(&boot_path) {
            return Ok(file);
        }
    }

    Err(FsError::NotFound)
}

/// Load kernel and optionally initrd from the shared filesystem.
pub fn load_boot_files(
    filesystem: &Rc<RefCell<Box<dyn Filesystem>>>,
    kernel_path: &str,
    initrd_path: Option<&str>,
) -> Result<(Vec<u8>, Option<Vec<u8>>), SpeedbootError> {
    let mut fs = filesystem.borrow_mut();

    let mut kernel_file =
        try_open_boot_file(&mut **fs, kernel_path).map_err(|_| SpeedbootError::FileNotFound)?;

    let kernel_data = kernel_file
        .read_to_end()
        .map_err(|_| SpeedbootError::FileReadError)?;

    let initrd_data = if let Some(initrd_path) = initrd_path {
        Some(
            try_open_boot_file(&mut **fs, initrd_path)
                .map_err(|_| SpeedbootError::FileNotFound)?
                .read_to_end()
                .map_err(|_| SpeedbootError::FileReadError)?,
        )
    } else {
        None
    };

    Ok((kernel_data, initrd_data))
}

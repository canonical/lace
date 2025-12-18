// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Julian Andres Klode <julian.klode@canonical.com>
//! Boot flow discovery and management.

pub mod bls;
pub mod grub;

use crate::SpeedbootError;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use lace_platform::fs::Filesystem;
use lace_platform::linux::boot_linux;
use lace_platform::println;

/// Trait for boot configurations.
pub trait BootConfiguration {
    /// Get the title of this boot configuration.
    fn title(&self) -> &str;

    /// Boot this configuration.
    fn start(self: Box<Self>) -> Result<(), SpeedbootError>;
}

/// Simple file-based boot configuration.
#[derive(Clone)]
pub struct SimpleBootConfiguration {
    pub title: String,
    linux: Option<String>,
    initrd: Option<String>,
    cmdline: Option<String>,
    /// Shared reference to the filesystem containing the boot files
    filesystem: Rc<RefCell<Box<dyn Filesystem>>>,
}

impl SimpleBootConfiguration {
    /// Create a new simple boot configuration.
    pub fn new(
        title: String,
        linux: Option<String>,
        initrd: Option<String>,
        cmdline: Option<String>,
        filesystem: Rc<RefCell<Box<dyn Filesystem>>>,
    ) -> Self {
        Self {
            title,
            linux,
            initrd,
            cmdline,
            filesystem,
        }
    }
}

impl BootConfiguration for SimpleBootConfiguration {
    fn title(&self) -> &str {
        &self.title
    }

    fn start(self: Box<Self>) -> Result<(), SpeedbootError> {
        println!("\nBooting: {}", self.title);

        if let Some(ref linux_path) = self.linux {
            println!("  Kernel: {}", linux_path);
        }
        if let Some(ref initrd) = self.initrd {
            println!("  Initrd: {}", initrd);
        }
        if let Some(ref cmdline) = self.cmdline {
            println!("  Cmdline: {}", cmdline);
        }

        // Load kernel and initrd from filesystem
        let (kernel_data, initrd_data) = grub::load_boot_files(
            &self.filesystem,
            self.linux.as_deref().ok_or(SpeedbootError::NoKernelPath)?,
            self.initrd.as_deref(),
        )?;

        // Boot Linux
        println!("  Starting kernel...");

        match lace_stubble::boot_stubble_image(
            lace_stubble::StubbleImage::Raw(&kernel_data),
            initrd_data.as_deref(),
            self.cmdline.as_deref(),
        ) {
            Err(lace_stubble::BootStubbleError::NotAStubbleImage) => boot_linux(
                &kernel_data,
                initrd_data.as_deref(),
                self.cmdline.as_deref(),
            )
            .map_err(|e| SpeedbootError::BootError(alloc::format!("{}", e))),
            Err(e) => Err(SpeedbootError::BootError(alloc::format!("{}", e))),
            Ok(_) => Ok(()),
        }?;

        Ok(())
    }
}

/// Trait for boot flow discovery mechanisms.
pub trait BootFlow {
    /// Discover boot configurations from a specific filesystem.
    fn discover(
        &self,
        filesystem: Rc<RefCell<Box<dyn Filesystem>>>,
    ) -> Result<Vec<Box<dyn BootConfiguration>>, SpeedbootError>;
}

/// Discover boot configurations from all available boot flows.
pub fn discover_all() -> Result<Vec<Box<dyn BootConfiguration>>, SpeedbootError> {
    let filesystems = lace_platform::fs::probe_all();

    let mut all_configs = Vec::new();

    println!(
        "Scanning {} filesystems for boot configurations...",
        filesystems.len()
    );

    for fs in filesystems {
        let fs_rc = Rc::new(RefCell::new(fs));

        // Try BLS boot flow
        let bls_flow = bls::BlsBootFlow::new();
        if let Ok(mut configs) = bls_flow.discover(Rc::clone(&fs_rc)) {
            all_configs.append(&mut configs);
        }

        // Try GRUB boot flow
        let grub_flow = grub::GrubBootFlow::new();
        if let Ok(mut configs) = grub_flow.discover(Rc::clone(&fs_rc)) {
            all_configs.append(&mut configs);
        }
    }

    if all_configs.is_empty() {
        return Err(SpeedbootError::NoBootEntriesFound);
    }

    println!("Found {} boot configurations total\n", all_configs.len());

    Ok(all_configs)
}

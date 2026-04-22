// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Julian Andres Klode <julian.klode@canonical.com>
//! Boot flow discovery and management.

pub mod bls;
pub mod grub;
pub mod speedboot;

use crate::SpeedbootError;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use lace_platform::fs::Filesystem;
use lace_platform::linux::boot_linux;

/// Trait for boot configurations.
pub trait BootConfiguration {
    /// Get the title of this boot configuration.
    fn title(&self) -> &str;

    /// OS machine-id this entry belongs to, if the discovering flow
    /// knew one. Used by `discover_all` to float the primary OS
    /// (matching `speedboot.toml`'s `primary-machine-id`) to the top
    /// of the menu.
    fn machine_id(&self) -> Option<&str> {
        None
    }

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
        log::info!("Booting: {}", self.title);
        if let Some(ref linux_path) = self.linux {
            log::debug!("Kernel: {}", linux_path);
        }
        if let Some(ref initrd) = self.initrd {
            log::debug!("Initrd: {}", initrd);
        }
        if let Some(ref cmdline) = self.cmdline {
            log::debug!("Cmdline: {}", cmdline);
        }

        // Load kernel and initrd from filesystem
        let (kernel_data, initrd_data) = grub::load_boot_files(
            &self.filesystem,
            self.linux.as_deref().ok_or(SpeedbootError::NoKernelPath)?,
            self.initrd.as_deref(),
        )?;

        // Boot Linux
        log::debug!("Starting kernel");

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

/// Optional primary-OS hint from the platform-provided
/// `speedboot.toml`.
#[derive(serde::Deserialize, Default)]
struct SpeedbootToml {
    #[serde(default)]
    speedboot: SpeedbootSection,
}

#[derive(serde::Deserialize, Default)]
struct SpeedbootSection {
    #[serde(rename = "primary-machine-id", default)]
    primary_machine_id: Option<String>,
}

fn primary_machine_id() -> Option<String> {
    let bytes = lace_platform::speedboot_toml()?;
    let text = core::str::from_utf8(&bytes).ok()?;
    let parsed: SpeedbootToml = toml::from_str(text)
        .inspect_err(|e| log::warn!("speedboot.toml parse error: {}", e))
        .ok()?;
    parsed.speedboot.primary_machine_id
}

/// Discover boot configurations from all available boot flows.
pub fn discover_all() -> Result<Vec<Box<dyn BootConfiguration>>, SpeedbootError> {
    let filesystems = lace_platform::fs::probe_all();
    let primary = primary_machine_id();

    let mut all_configs = Vec::new();

    log::debug!(
        "Scanning {} filesystems for boot configurations",
        filesystems.len()
    );

    // Boot flows in descending priority. For each filesystem we
    // pick the first flow that actually produces entries and stop —
    // a speedboot-native `boot.toml` wins over a GRUB config on the
    // same partition, etc.
    let flows: &[&dyn BootFlow] = &[
        &speedboot::SpeedbootBootFlow::new(),
        &bls::BlsBootFlow::new(),
        &grub::GrubBootFlow::new(),
    ];

    for fs in filesystems {
        let fs_rc = Rc::new(RefCell::new(fs));
        for flow in flows {
            if let Ok(mut configs) = flow.discover(Rc::clone(&fs_rc)) {
                all_configs.append(&mut configs);
                break;
            }
        }
    }

    if all_configs.is_empty() {
        return Err(SpeedbootError::NoBootEntriesFound);
    }

    // Stable-sort so entries matching the primary machine-id float to
    // the top in their original order.
    if let Some(primary) = primary.as_deref() {
        all_configs.sort_by_key(|cfg| if cfg.machine_id() == Some(primary) { 0 } else { 1 });
    }

    log::debug!("Found {} boot configurations total", all_configs.len());

    Ok(all_configs)
}

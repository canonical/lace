// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! Speedboot-native boot flow (`flow = "classic-1"`).
//!
//! Discovery model:
//! - Each OS drops a `boot.toml` at `/boot.toml` or `/boot/boot.toml`
//!   on its boot partition. `[boot].flow = "classic-1"` means
//!   "kernels and initrds sit next to me, named `vmlinuz-<ver>` and
//!   `initrd.img-<ver>`".
//! - `[os]` carries `/etc/os-release`-style metadata plus
//!   `machine-id`.
//! - `[[profile]]` describes what entries to emit per kernel. The
//!   single profile with no `label` is the default. Others carry a
//!   free-form `label` like `"recovery"`.
//!
//! Menu shape per OS:
//! - One synthetic head entry `<pretty-name>` = default profile ×
//!   newest kernel.
//! - Then, newest-first per kernel: `<pretty-name> - kernel <ver>`
//!   (default profile) followed by
//!   `<pretty-name> - kernel <ver>, <label>` for each labelled
//!   profile.

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use lace_platform::fs::Filesystem;
use lace_platform::linux::boot_linux;
use lace_util::kversion::KernelVersion;
use serde::Deserialize;

use super::{BootConfiguration, BootFlow, grub};
use crate::SpeedbootError;

const CLASSIC_1: &str = "classic-1";
const BOOT_TOML_PATHS: &[(&str, &str)] = &[("/boot.toml", "/"), ("/boot/boot.toml", "/boot")];
const VMLINUZ_PREFIX: &str = "vmlinuz-";
const INITRD_PREFIX: &str = "initrd.img-";

#[derive(Debug, Deserialize)]
struct BootToml {
    boot: BootSection,
    os: OsSection,
    #[serde(default)]
    profile: Vec<ProfileSection>,
}

#[derive(Debug, Deserialize)]
struct BootSection {
    flow: String,
}

#[derive(Debug, Deserialize)]
struct OsSection {
    #[serde(rename = "pretty-name")]
    pretty_name: String,
    #[serde(rename = "machine-id", default)]
    machine_id: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct ProfileSection {
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    cmdline: Option<String>,
}

/// Speedboot `classic-1` boot flow.
pub struct SpeedbootBootFlow;

impl SpeedbootBootFlow {
    pub fn new() -> Self {
        Self
    }
}

impl BootFlow for SpeedbootBootFlow {
    fn discover(
        &self,
        filesystem: Rc<RefCell<Box<dyn Filesystem>>>,
    ) -> Result<Vec<Box<dyn BootConfiguration>>, SpeedbootError> {
        // Locate boot.toml.
        let (toml_bytes, boot_dir) = {
            let mut found = None;
            for (path, dir) in BOOT_TOML_PATHS {
                let mut fs = filesystem.borrow_mut();
                if let Ok(mut f) = fs.open_file(path)
                    && let Ok(buf) = f.read_to_end()
                {
                    found = Some((buf, *dir));
                    break;
                }
            }
            match found {
                Some(x) => x,
                None => return Err(SpeedbootError::NoBootEntriesFound),
            }
        };

        let text = core::str::from_utf8(&toml_bytes)
            .map_err(|_| SpeedbootError::NoBootEntriesFound)?;
        let parsed: BootToml = toml::from_str(text).map_err(|e| {
            log::debug!("speedboot: boot.toml parse error: {}", e);
            SpeedbootError::NoBootEntriesFound
        })?;

        if parsed.boot.flow != CLASSIC_1 {
            log::debug!(
                "speedboot: unsupported flow {:?} in {}",
                parsed.boot.flow,
                boot_dir
            );
            return Err(SpeedbootError::NoBootEntriesFound);
        }

        // Split profiles: exactly one default (unlabeled), plus any
        // labelled ones in file order.
        let mut default_profile: Option<ProfileSection> = None;
        let mut labelled: Vec<ProfileSection> = Vec::new();
        for p in parsed.profile {
            if p.label.is_none() {
                if default_profile.is_none() {
                    default_profile = Some(p);
                } else {
                    log::warn!("speedboot: multiple unlabeled profiles, keeping the first");
                }
            } else {
                labelled.push(p);
            }
        }

        // Enumerate kernels in the boot directory.
        let mut kernels: Vec<(KernelVersion, bool)> = Vec::new();
        let entries = filesystem
            .borrow_mut()
            .read_dir(boot_dir)
            .map_err(|_| SpeedbootError::NoBootEntriesFound)?;
        for entry in &entries {
            if entry.is_dir {
                continue;
            }
            let Some(suffix) = entry.name.strip_prefix(VMLINUZ_PREFIX) else {
                continue;
            };
            let Some(ver) = KernelVersion::parse(suffix) else {
                log::debug!("speedboot: skipping unparseable kernel {:?}", entry.name);
                continue;
            };
            let initrd_name = format!("{}{}", INITRD_PREFIX, suffix);
            let has_initrd = entries.iter().any(|e| e.name == initrd_name);
            kernels.push((ver, has_initrd));
        }
        if kernels.is_empty() {
            return Err(SpeedbootError::NoBootEntriesFound);
        }
        kernels.sort_by(|a, b| b.0.cmp(&a.0));

        let machine_id = parsed.os.machine_id.as_deref();
        let mut out: Vec<Box<dyn BootConfiguration>> = Vec::new();

        // Synthetic head entry: default profile × newest kernel.
        if let Some(def) = default_profile.as_ref() {
            let (newest, has_initrd) = &kernels[0];
            out.push(make_entry(
                parsed.os.pretty_name.clone(),
                boot_dir,
                newest,
                *has_initrd,
                def.cmdline.as_deref(),
                machine_id,
                &filesystem,
            ));
        }

        for (ver, has_initrd) in &kernels {
            if let Some(def) = default_profile.as_ref() {
                let title = format!("{} - kernel {}", parsed.os.pretty_name, ver.raw);
                out.push(make_entry(
                    title,
                    boot_dir,
                    ver,
                    *has_initrd,
                    def.cmdline.as_deref(),
                    machine_id,
                    &filesystem,
                ));
            }
            for p in &labelled {
                let label = p.label.as_deref().unwrap_or("");
                let title = format!(
                    "{} - kernel {}, {}",
                    parsed.os.pretty_name, ver.raw, label
                );
                out.push(make_entry(
                    title,
                    boot_dir,
                    ver,
                    *has_initrd,
                    p.cmdline.as_deref(),
                    machine_id,
                    &filesystem,
                ));
            }
        }

        if out.is_empty() {
            return Err(SpeedbootError::NoBootEntriesFound);
        }
        Ok(out)
    }
}

fn make_entry(
    title: String,
    boot_dir: &str,
    ver: &KernelVersion,
    has_initrd: bool,
    cmdline: Option<&str>,
    machine_id: Option<&str>,
    filesystem: &Rc<RefCell<Box<dyn Filesystem>>>,
) -> Box<dyn BootConfiguration> {
    let sep = if boot_dir.ends_with('/') { "" } else { "/" };
    let linux_path = format!("{}{}{}{}", boot_dir, sep, VMLINUZ_PREFIX, ver.raw);
    let initrd_path = if has_initrd {
        Some(format!("{}{}{}{}", boot_dir, sep, INITRD_PREFIX, ver.raw))
    } else {
        None
    };
    Box::new(SpeedbootBootConfiguration {
        title,
        linux: linux_path,
        initrd: initrd_path,
        cmdline: cmdline.map(|s| s.to_string()),
        machine_id: machine_id.map(|s| s.to_string()),
        filesystem: Rc::clone(filesystem),
    })
}

/// One `classic-1` boot entry.
struct SpeedbootBootConfiguration {
    title: String,
    linux: String,
    initrd: Option<String>,
    cmdline: Option<String>,
    machine_id: Option<String>,
    filesystem: Rc<RefCell<Box<dyn Filesystem>>>,
}

impl BootConfiguration for SpeedbootBootConfiguration {
    fn title(&self) -> &str {
        &self.title
    }

    fn machine_id(&self) -> Option<&str> {
        self.machine_id.as_deref()
    }

    fn start(self: Box<Self>) -> Result<(), SpeedbootError> {
        log::info!("Booting: {}", self.title);
        log::debug!("Kernel: {}", self.linux);
        if let Some(ref initrd) = self.initrd {
            log::debug!("Initrd: {}", initrd);
        }
        if let Some(ref cmdline) = self.cmdline {
            log::debug!("Cmdline: {}", cmdline);
        }

        let (kernel_data, initrd_data) =
            grub::load_boot_files(&self.filesystem, &self.linux, self.initrd.as_deref())?;

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
        }
    }
}

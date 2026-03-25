// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.

//! Filesystem probing and discovery.
//!
//! Discovers block devices (via platform code), probes partition tables
//! (GPT/MBR), and mounts known filesystems.

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;

use super::base::{BlockDevice, Filesystem};
use super::gpt;
use super::mbr;

// ---------------------------------------------------------------------------
// Partition sub-device
// ---------------------------------------------------------------------------

/// A partition-scoped view of a shared block device.
struct PartitionBlockDevice {
    inner: Rc<RefCell<Box<dyn BlockDevice>>>,
    sector_size: u32,
    start_sector: u64,
    size_sectors: u64,
}

impl BlockDevice for PartitionBlockDevice {
    fn read_sectors(
        &mut self,
        lba: u64,
        count: u32,
        buf: &mut [u8],
    ) -> Result<(), super::base::FsError> {
        if lba
            .checked_add(count as u64)
            .is_none_or(|end_lba| end_lba > self.size_sectors)
        {
            return Err(super::base::FsError::Invalid);
        }

        let abs_lba = match self.start_sector.checked_add(lba) {
            Some(v) => v,
            None => return Err(super::base::FsError::Invalid),
        };

        self.inner.borrow_mut().read_sectors(abs_lba, count, buf)
    }

    fn sector_size(&self) -> u32 {
        self.sector_size
    }

    fn sector_count(&self) -> u64 {
        self.size_sectors
    }
}

// ---------------------------------------------------------------------------
// Platform discovery result
// ---------------------------------------------------------------------------

/// Result of platform storage discovery.
///
/// Platforms populate this with whatever they find. The probe layer then
/// does partition table probing on whole disks and filesystem probing on
/// partitions.
pub struct DiscoveredStorage {
    /// Whole-disk block devices (will be probed for GPT/MBR).
    pub disks: Vec<Box<dyn BlockDevice>>,
    /// Already partition-resolved block devices (will be probed for filesystems).
    pub partitions: Vec<Box<dyn BlockDevice>>,
    /// Pre-mounted filesystems (returned as-is).
    pub filesystems: Vec<Box<dyn Filesystem>>,
}

impl Default for DiscoveredStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl DiscoveredStorage {
    /// Create an empty result.
    pub fn new() -> Self {
        Self {
            disks: Vec::new(),
            partitions: Vec::new(),
            filesystems: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Filesystem probing
// ---------------------------------------------------------------------------

/// Try to mount a known filesystem on a block device.
fn try_mount_filesystem(dev: Box<dyn BlockDevice>) -> Option<Box<dyn Filesystem>> {
    #[cfg(feature = "ext4")]
    {
        use super::ext4::Ext4Filesystem;
        if let Ok(fs) = Ext4Filesystem::new(dev) {
            return Some(Box::new(fs));
        }
    }

    let _ = dev;
    None
}

/// Probe a whole disk for partitions (GPT/MBR) and try mounting filesystems.
fn probe_disk(dev: Box<dyn BlockDevice>) -> Vec<Box<dyn Filesystem>> {
    let sector_size = dev.sector_size();
    let sector_count = dev.sector_count();

    // Wrap in Rc<RefCell<...>> so partition sub-devices can share the underlying
    // disk in this single-threaded context. We store the Box directly inside the
    // RefCell — PartitionBlockDevice accesses it through the shared Rc.
    let shared: Rc<RefCell<Box<dyn BlockDevice>>> = Rc::new(RefCell::new(dev));

    let mut whole_disk = PartitionBlockDevice {
        inner: Rc::clone(&shared),
        sector_size,
        start_sector: 0,
        size_sectors: sector_count,
    };

    struct FoundPartition {
        start_lba: u64,
        size_lba: u64,
    }

    let found: Option<(&str, Vec<FoundPartition>)> =
        if let Ok(parts) = gpt::parse_gpt(&mut whole_disk, sector_size) {
            Some((
                "GPT",
                parts
                    .iter()
                    .map(|p| FoundPartition {
                        start_lba: p.start_lba,
                        size_lba: p.size_lba,
                    })
                    .collect(),
            ))
        } else if let Ok(parts) = mbr::parse_mbr(&mut whole_disk) {
            Some((
                "MBR",
                parts
                    .iter()
                    .map(|p| FoundPartition {
                        start_lba: p.start_lba,
                        size_lba: p.size_lba,
                    })
                    .collect(),
            ))
        } else {
            None
        };

    let mut results = Vec::new();

    if let Some((scheme, partitions)) = found {
        crate::debugln!("[fs] Found {} with {} partitions", scheme, partitions.len());
        for part in &partitions {
            let part_dev = Box::new(PartitionBlockDevice {
                inner: Rc::clone(&shared),
                sector_size,
                start_sector: part.start_lba,
                size_sectors: part.size_lba,
            });
            if let Some(fs) = try_mount_filesystem(part_dev) {
                results.push(fs);
            }
        }
    } else {
        // No partition table -- try whole disk as a filesystem
        let whole_dev = Box::new(PartitionBlockDevice {
            inner: shared,
            sector_size,
            start_sector: 0,
            size_sectors: sector_count,
        });
        if let Some(fs) = try_mount_filesystem(whole_dev) {
            results.push(fs);
        }
    }

    results
}

/// Process a [`DiscoveredStorage`] into a list of mounted filesystems.
fn process(discovered: DiscoveredStorage) -> Vec<Box<dyn Filesystem>> {
    let mut results = discovered.filesystems;

    for dev in discovered.disks {
        results.extend(probe_disk(dev));
    }

    for dev in discovered.partitions {
        if let Some(fs) = try_mount_filesystem(dev) {
            results.push(fs);
        }
    }

    crate::debugln!("[fs] Probed {} filesystems total", results.len());
    results
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Discover all storage and return mounted filesystems.
pub fn probe_all() -> Vec<Box<dyn Filesystem>> {
    process(crate::p::fs::discover_storage())
}

/// Discover storage on the boot disk only and return mounted filesystems.
pub fn probe_boot_device() -> Vec<Box<dyn Filesystem>> {
    process(crate::p::fs::discover_boot_storage())
}

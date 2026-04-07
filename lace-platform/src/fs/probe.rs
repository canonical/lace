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

#[cfg(all(test, feature = "ext4"))]
mod test {
    use super::*;
    use crate::fs::ext4::test::make_ext4_image;
    use crate::fs::gpt::test::build_gpt_disk_with_data;
    use crate::fs::mbr::test::build_mbr_disk_with_data;
    use lace_util::count_blocks_aligned_up;

    // -- try_mount_filesystem --

    #[test]
    fn test_try_mount_filesystem_valid_ext4() {
        let image = make_ext4_image(&[("/hello.txt", "hi".as_bytes())]);
        let sector_size = 512u32;
        let sector_count = count_blocks_aligned_up!(image.len() as u64, sector_size as u64);
        let mut disk = crate::fs::testutil::MemDisk::new(sector_size, sector_count);
        disk.write_at(0, &image);

        let result = try_mount_filesystem(Box::new(disk));
        assert!(result.is_some(), "should mount a valid ext4 image");
    }

    #[test]
    fn test_try_mount_filesystem_invalid_data() {
        let garbage = vec![0xFFu8; 1024 * 1024];
        let sector_size = 512u32;
        let sector_count = count_blocks_aligned_up!(garbage.len() as u64, sector_size as u64);
        let mut disk = crate::fs::testutil::MemDisk::new(sector_size, sector_count);
        disk.write_at(0, &garbage);

        let result = try_mount_filesystem(Box::new(disk));
        assert!(result.is_none(), "garbage data should not mount");
    }

    // -- probe_disk with GPT --

    #[test]
    fn test_probe_disk_gpt_one_ext4_partition() {
        let ext4_data = make_ext4_image(&[("/test.txt", "gpt works".as_bytes())]);
        let disk = build_gpt_disk_with_data(&[&ext4_data]);

        let mut filesystems = probe_disk(Box::new(disk));
        assert_eq!(filesystems.len(), 1, "expected one mounted filesystem");

        let data = filesystems[0]
            .open_file("/test.txt")
            .unwrap()
            .read_to_end()
            .unwrap();
        assert_eq!(data, "gpt works".as_bytes());
    }

    #[test]
    fn test_probe_disk_gpt_two_ext4_partitions() {
        let ext4_a = make_ext4_image(&[("/a.txt", "partition-a".as_bytes())]);
        let ext4_b = make_ext4_image(&[("/b.txt", "partition-b".as_bytes())]);
        let disk = build_gpt_disk_with_data(&[&ext4_a, &ext4_b]);

        let mut filesystems = probe_disk(Box::new(disk));
        assert_eq!(filesystems.len(), 2, "expected two mounted filesystems");

        let data_a = filesystems[0]
            .open_file("/a.txt")
            .unwrap()
            .read_to_end()
            .unwrap();
        assert_eq!(data_a, "partition-a".as_bytes());

        let data_b = filesystems[1]
            .open_file("/b.txt")
            .unwrap()
            .read_to_end()
            .unwrap();
        assert_eq!(data_b, "partition-b".as_bytes());
    }

    #[test]
    fn test_probe_disk_gpt_mixed_valid_and_invalid() {
        let ext4_data = make_ext4_image(&[("/good.txt", "ok".as_bytes())]);
        let garbage = vec![0xFFu8; 512 * 2048]; // 1 MiB of garbage
        let disk = build_gpt_disk_with_data(&[&garbage, &ext4_data]);

        let mut filesystems = probe_disk(Box::new(disk));
        assert_eq!(
            filesystems.len(),
            1,
            "only the valid ext4 partition should mount"
        );

        let data = filesystems[0]
            .open_file("/good.txt")
            .unwrap()
            .read_to_end()
            .unwrap();
        assert_eq!(data, "ok".as_bytes());
    }

    // -- probe_disk with MBR --

    #[test]
    fn test_probe_disk_mbr_one_ext4_partition() {
        let ext4_data = make_ext4_image(&[("/mbr.txt", "mbr works".as_bytes())]);
        let disk = build_mbr_disk_with_data(&[&ext4_data]);

        let mut filesystems = probe_disk(Box::new(disk));
        assert_eq!(filesystems.len(), 1, "expected one mounted filesystem");

        let data = filesystems[0]
            .open_file("/mbr.txt")
            .unwrap()
            .read_to_end()
            .unwrap();
        assert_eq!(data, "mbr works".as_bytes());
    }

    #[test]
    fn test_probe_disk_mbr_two_ext4_partitions() {
        let ext4_a = make_ext4_image(&[("/first.txt", "one".as_bytes())]);
        let ext4_b = make_ext4_image(&[("/second.txt", "two".as_bytes())]);
        let disk = build_mbr_disk_with_data(&[&ext4_a, &ext4_b]);

        let mut filesystems = probe_disk(Box::new(disk));
        assert_eq!(filesystems.len(), 2, "expected two mounted filesystems");

        let data_a = filesystems[0]
            .open_file("/first.txt")
            .unwrap()
            .read_to_end()
            .unwrap();
        assert_eq!(data_a, "one".as_bytes());

        let data_b = filesystems[1]
            .open_file("/second.txt")
            .unwrap()
            .read_to_end()
            .unwrap();
        assert_eq!(data_b, "two".as_bytes());
    }

    // -- probe_disk: whole disk fallback (no partition table) --

    #[test]
    fn test_probe_disk_no_partition_table_ext4() {
        let ext4_data = make_ext4_image(&[("/bare.txt", "bare disk".as_bytes())]);
        let sector_size = 512u32;
        let sector_count = count_blocks_aligned_up!(ext4_data.len() as u64, sector_size as u64);
        let mut disk = crate::fs::testutil::MemDisk::new(sector_size, sector_count);
        disk.write_at(0, &ext4_data);

        let mut filesystems = probe_disk(Box::new(disk));
        assert_eq!(filesystems.len(), 1, "should fall back to whole-disk mount");

        let data = filesystems[0]
            .open_file("/bare.txt")
            .unwrap()
            .read_to_end()
            .unwrap();
        assert_eq!(data, "bare disk".as_bytes());
    }

    #[test]
    fn test_probe_disk_no_partition_table_no_filesystem() {
        let garbage = vec![0xFFu8; 1024 * 1024];
        let sector_size = 512u32;
        let sector_count = count_blocks_aligned_up!(garbage.len() as u64, sector_size as u64);
        let mut disk = crate::fs::testutil::MemDisk::new(sector_size, sector_count);
        disk.write_at(0, &garbage);

        let filesystems = probe_disk(Box::new(disk));
        assert!(
            filesystems.is_empty(),
            "garbage disk should yield no filesystems"
        );
    }

    // -- process (DiscoveredStorage) --

    #[test]
    fn test_process_with_disk_and_partition() {
        let ext4_disk_img = make_ext4_image(&[("/from_disk.txt", "disk".as_bytes())]);
        let disk = build_gpt_disk_with_data(&[&ext4_disk_img]);

        let ext4_part_img = make_ext4_image(&[("/from_part.txt", "part".as_bytes())]);
        let sector_size = 512u32;
        let sector_count = count_blocks_aligned_up!(ext4_part_img.len() as u64, sector_size as u64);
        let mut part_dev = crate::fs::testutil::MemDisk::new(sector_size, sector_count);
        part_dev.write_at(0, &ext4_part_img);

        let discovered = DiscoveredStorage {
            disks: vec![Box::new(disk)],
            partitions: vec![Box::new(part_dev)],
            filesystems: Vec::new(),
        };

        let mut results = process(discovered);
        assert_eq!(results.len(), 2, "one from disk probe, one from partition");

        let from_disk = results[0]
            .open_file("/from_disk.txt")
            .unwrap()
            .read_to_end()
            .unwrap();
        assert_eq!(from_disk, "disk".as_bytes());

        let from_part = results[1]
            .open_file("/from_part.txt")
            .unwrap()
            .read_to_end()
            .unwrap();
        assert_eq!(from_part, "part".as_bytes());
    }

    #[test]
    fn test_process_empty_discovery() {
        let discovered = DiscoveredStorage::new();
        let results = process(discovered);
        assert!(results.is_empty());
    }

    #[test]
    fn test_process_preserves_pre_mounted_filesystems() {
        // Create a pre-mounted filesystem via ext4 directly
        let ext4_img = make_ext4_image(&[("/premounted.txt", "pre".as_bytes())]);
        let sector_size = 512u32;
        let sector_count = count_blocks_aligned_up!(ext4_img.len() as u64, sector_size as u64);
        let mut disk = crate::fs::testutil::MemDisk::new(sector_size, sector_count);
        disk.write_at(0, &ext4_img);
        let pre_fs = crate::fs::ext4::Ext4Filesystem::new(Box::new(disk)).unwrap();

        let discovered = DiscoveredStorage {
            disks: Vec::new(),
            partitions: Vec::new(),
            filesystems: vec![Box::new(pre_fs)],
        };

        let mut results = process(discovered);
        assert_eq!(results.len(), 1);

        let data = results[0]
            .open_file("/premounted.txt")
            .unwrap()
            .read_to_end()
            .unwrap();
        assert_eq!(data, "pre".as_bytes());
    }
}

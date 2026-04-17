// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! Virt platform storage discovery via virtio-blk

use crate::fs::base::{BlockDevice, FsError};
use crate::fs::probe::DiscoveredStorage;
use alloc::boxed::Box;
use lace_drivers::pci::{self, Ecam};
use lace_drivers::virtio::blk::VirtioBlkDevice;

// Virtio PCI vendor ID
const VIRTIO_VENDOR_ID: u16 = 0x1AF4;
// Virtio-blk modern device ID
const VIRTIO_BLK_DEVICE_ID: u16 = 0x1042;

/// Wrapper that adapts a [`VirtioBlkDevice`] to the [`BlockDevice`] trait.
struct VirtioBlockDevice(VirtioBlkDevice);

impl BlockDevice for VirtioBlockDevice {
    fn read_sectors(&mut self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), FsError> {
        self.0
            .read_sectors(lba, &mut buf[..count as usize * 512])
            .map_err(|_| FsError::Io(crate::Error::Virtio))
    }

    fn sector_size(&self) -> u32 {
        512
    }

    fn sector_count(&self) -> u64 {
        self.0.capacity()
    }

    fn block_size(&self) -> u32 {
        self.0.block_size()
    }
}

/// Discover all virtio-blk devices on the PCI bus.
fn discover_virtio_blk(ecam: &Ecam) -> DiscoveredStorage {
    let mut storage = DiscoveredStorage::new();

    let devices = pci::enumerate_bus(ecam, 0);
    for dev in &devices {
        if dev.vendor_id == VIRTIO_VENDOR_ID && dev.device_id == VIRTIO_BLK_DEVICE_ID {
            log::debug!(
                "virtio-blk at PCI {:02x}:{:02x}.{}",
                dev.bus,
                dev.dev,
                dev.func
            );
            if let Some(blk) = VirtioBlkDevice::new(ecam, dev) {
                log::debug!(
                    "  capacity: {} sectors, block size: {}",
                    blk.capacity(),
                    blk.block_size()
                );
                storage.disks.push(Box::new(VirtioBlockDevice(blk)));
            }
        }
    }

    storage
}

/// Global ECAM base, set during platform init.
static mut ECAM_BASE: u64 = 0;

/// Called from platform init to store the ECAM base address.
pub fn set_ecam_base(base: u64) {
    unsafe { ECAM_BASE = base };
}

pub fn discover_storage() -> DiscoveredStorage {
    let ecam_base = unsafe { ECAM_BASE };
    if ecam_base != 0 {
        discover_virtio_blk(&Ecam::new(ecam_base))
    } else {
        DiscoveredStorage::new()
    }
}

pub fn discover_boot_storage() -> DiscoveredStorage {
    discover_storage()
}

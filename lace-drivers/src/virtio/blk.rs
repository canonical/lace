// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! Virtio block device driver

use alloc::boxed::Box;

use crate::pci::{Ecam, PciDevice};

use super::{VRING_DESC_F_NEXT, VRING_DESC_F_WRITE, VirtioDevice};

// Virtio-blk request types
const VIRTIO_BLK_T_IN: u32 = 0; // Read
#[allow(dead_code)]
const VIRTIO_BLK_T_OUT: u32 = 1; // Write

// Virtio-blk status codes
const VIRTIO_BLK_S_OK: u8 = 0;

// Virtio-blk feature bits
const VIRTIO_BLK_F_BLK_SIZE: u64 = 1 << 6;

// Virtio-blk config offsets (all little-endian)
const CFG_CAPACITY: usize = 0; // u64: capacity in 512-byte sectors
const CFG_BLK_SIZE: usize = 20; // u32: block size (if feature negotiated)

/// Virtio-blk request header.
#[repr(C)]
struct VirtioBlkReqHeader {
    type_: u32,
    _reserved: u32,
    sector: u64,
}

/// Error from a virtio-blk operation.
#[derive(Debug)]
pub struct VirtioBlkError;

/// Virtio block device.
pub struct VirtioBlkDevice {
    dev: VirtioDevice,
    capacity: u64,
    blk_size: u32,
}

impl VirtioBlkDevice {
    /// Initialize a virtio-blk device from a PCI device.
    pub fn new(ecam: &Ecam, pci_dev: &PciDevice) -> Option<Self> {
        let dev = VirtioDevice::new(ecam, pci_dev, VIRTIO_BLK_F_BLK_SIZE)?;

        let capacity: u64 = unsafe { dev.read_device_config(CFG_CAPACITY) };
        let blk_size: u32 = unsafe { dev.read_device_config(CFG_BLK_SIZE) };
        let blk_size = if blk_size == 0 { 512 } else { blk_size };

        Some(Self {
            dev,
            capacity,
            blk_size,
        })
    }

    /// Capacity in 512-byte sectors.
    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    /// Physical block size in bytes.
    pub fn block_size(&self) -> u32 {
        self.blk_size
    }

    /// Read sectors (512 bytes each) starting at the given LBA into `buf`.
    pub fn read_sectors(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), VirtioBlkError> {
        let vq = self.dev.queue_mut(0);

        let hdr_desc = vq.alloc_desc();
        let data_desc = vq.alloc_desc();
        let status_desc = vq.alloc_desc();

        let header = VirtioBlkReqHeader {
            type_: VIRTIO_BLK_T_IN,
            _reserved: 0,
            sector,
        };
        let header_box = Box::new(header);
        let header_ptr = Box::into_raw(header_box);

        let mut status: u8 = 0xFF;

        unsafe {
            let desc_base = vq.desc;

            let d = &mut *desc_base.add(hdr_desc as usize);
            d.addr = header_ptr as u64;
            d.len = size_of::<VirtioBlkReqHeader>() as u32;
            d.flags = VRING_DESC_F_NEXT;
            d.next = data_desc;

            let d = &mut *desc_base.add(data_desc as usize);
            d.addr = buf.as_mut_ptr() as u64;
            d.len = buf.len() as u32;
            d.flags = VRING_DESC_F_WRITE | VRING_DESC_F_NEXT;
            d.next = status_desc;

            let d = &mut *desc_base.add(status_desc as usize);
            d.addr = &raw mut status as u64;
            d.len = 1;
            d.flags = VRING_DESC_F_WRITE;
            d.next = 0;
        }

        vq.submit(hdr_desc);
        let (_id, _len) = vq.poll_used();

        vq.free_desc(status_desc);
        vq.free_desc(data_desc);
        vq.free_desc(hdr_desc);

        let _ = unsafe { Box::from_raw(header_ptr) };

        if status != VIRTIO_BLK_S_OK {
            return Err(VirtioBlkError);
        }

        Ok(())
    }
}

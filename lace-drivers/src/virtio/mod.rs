// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! Virtio device driver (modern 1.0+ PCI transport)

use core::sync::atomic::{Ordering, fence};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::pci::{Ecam, PciDevice, read_bar_mmio64};

pub mod blk;

// Virtio PCI capability types
const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;

// Virtio device status bits
const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1;
const VIRTIO_STATUS_DRIVER: u8 = 2;
const VIRTIO_STATUS_DRIVER_OK: u8 = 4;
const VIRTIO_STATUS_FEATURES_OK: u8 = 8;

// Virtio feature bits
const VIRTIO_F_VERSION_1: u64 = 1 << 32;

// Vring descriptor flags
const VRING_DESC_F_NEXT: u16 = 1;
const VRING_DESC_F_WRITE: u16 = 2;

// PCI capability ID for vendor-specific
const PCI_CAP_ID_VNDR: u8 = 0x09;

/// Resolved MMIO addresses for a virtio PCI device's capability regions.
struct VirtioPciRegions {
    common: *mut u8,
    notify: *mut u8,
    notify_off_multiplier: u32,
    _isr: *mut u8,
    device: *mut u8,
}

/// Walk PCI capabilities to find virtio-specific regions.
fn find_virtio_pci_regions(ecam: &Ecam, dev: &PciDevice) -> Option<VirtioPciRegions> {
    let mut common = None;
    let mut notify = None;
    let mut notify_off_multiplier = 0u32;
    let mut _isr = None;
    let mut device = None;

    // PCI capability pointer is at offset 0x34
    let mut cap_offset = ecam.read_u8(dev.bus, dev.dev, dev.func, 0x34) as u16;

    while cap_offset != 0 {
        let cap_id = ecam.read_u8(dev.bus, dev.dev, dev.func, cap_offset);
        let cap_next = ecam.read_u8(dev.bus, dev.dev, dev.func, cap_offset + 1);

        if cap_id == PCI_CAP_ID_VNDR {
            let cfg_type = ecam.read_u8(dev.bus, dev.dev, dev.func, cap_offset + 3);
            let bar = ecam.read_u8(dev.bus, dev.dev, dev.func, cap_offset + 4);
            let offset = ecam.read_u32(dev.bus, dev.dev, dev.func, cap_offset + 8);

            // Resolve BAR base address
            let Some(bar_base) = read_bar_mmio64(ecam, dev, bar) else {
                log::debug!(
                    "virtio cap type={} bar={} - BAR not assigned",
                    cfg_type,
                    bar
                );
                cap_offset = cap_next as u16;
                continue;
            };
            let region_ptr = (bar_base + offset as u64) as *mut u8;

            match cfg_type {
                VIRTIO_PCI_CAP_COMMON_CFG => common = Some(region_ptr),
                VIRTIO_PCI_CAP_NOTIFY_CFG => {
                    notify = Some(region_ptr);
                    // notify_off_multiplier is at cap_offset + 16
                    notify_off_multiplier =
                        ecam.read_u32(dev.bus, dev.dev, dev.func, cap_offset + 16);
                }
                VIRTIO_PCI_CAP_ISR_CFG => _isr = Some(region_ptr),
                VIRTIO_PCI_CAP_DEVICE_CFG => device = Some(region_ptr),
                _ => {}
            }
        }

        cap_offset = cap_next as u16;
    }

    Some(VirtioPciRegions {
        common: common?,
        notify: notify?,
        notify_off_multiplier,
        _isr: _isr?,
        device: device?,
    })
}

// Common config register offsets (from virtio_pci_common_cfg)
const COMMON_DEVICE_FEATURE_SELECT: usize = 0;
const COMMON_DEVICE_FEATURE: usize = 4;
const COMMON_GUEST_FEATURE_SELECT: usize = 8;
const COMMON_GUEST_FEATURE: usize = 12;
#[allow(dead_code)]
const COMMON_MSIX_CONFIG: usize = 16;
const COMMON_NUM_QUEUES: usize = 18;
const COMMON_DEVICE_STATUS: usize = 20;
#[allow(dead_code)]
const COMMON_CONFIG_GENERATION: usize = 21;
const COMMON_QUEUE_SELECT: usize = 22;
const COMMON_QUEUE_SIZE: usize = 24;
const COMMON_QUEUE_MSIX_VECTOR: usize = 26;
const COMMON_QUEUE_ENABLE: usize = 28;
const COMMON_QUEUE_NOTIFY_OFF: usize = 30;
const COMMON_QUEUE_DESC_LO: usize = 32;
const COMMON_QUEUE_DESC_HI: usize = 36;
const COMMON_QUEUE_AVAIL_LO: usize = 40;
const COMMON_QUEUE_AVAIL_HI: usize = 44;
const COMMON_QUEUE_USED_LO: usize = 48;
const COMMON_QUEUE_USED_HI: usize = 52;

unsafe fn mmio_read_u8(base: *mut u8, offset: usize) -> u8 {
    unsafe { core::ptr::read_volatile(base.add(offset)) }
}
unsafe fn mmio_read_u16(base: *mut u8, offset: usize) -> u16 {
    unsafe { core::ptr::read_volatile(base.add(offset) as *const u16) }
}
unsafe fn mmio_read_u32(base: *mut u8, offset: usize) -> u32 {
    unsafe { core::ptr::read_volatile(base.add(offset) as *const u32) }
}
unsafe fn mmio_write_u8(base: *mut u8, offset: usize, val: u8) {
    unsafe { core::ptr::write_volatile(base.add(offset), val) }
}
unsafe fn mmio_write_u16(base: *mut u8, offset: usize, val: u16) {
    unsafe { core::ptr::write_volatile(base.add(offset) as *mut u16, val) }
}
unsafe fn mmio_write_u32(base: *mut u8, offset: usize, val: u32) {
    unsafe { core::ptr::write_volatile(base.add(offset) as *mut u32, val) }
}

/// Virtqueue descriptor.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
struct VringDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

/// Virtqueue available ring header.
#[repr(C)]
struct VringAvail {
    flags: u16,
    idx: u16,
    // ring: [u16; queue_size] follows
}

/// Virtqueue used ring header.
#[repr(C)]
struct VringUsed {
    flags: u16,
    idx: u16,
    // ring: [VringUsedElem; queue_size] follows
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VringUsedElem {
    id: u32,
    len: u32,
}

/// A single virtqueue.
pub struct VirtQueue {
    /// Queue size (number of descriptors).
    size: u16,
    /// Pointer to the descriptor table.
    desc: *mut VringDesc,
    /// Pointer to the available ring.
    avail: *mut VringAvail,
    /// Pointer to the used ring.
    used: *mut VringUsed,
    /// Next free descriptor index.
    free_head: u16,
    /// Last seen used index.
    last_used_idx: u16,
    /// Notify address for this queue.
    notify_addr: *mut u16,
}

impl VirtQueue {
    /// Allocate and initialize a virtqueue.
    fn new(size: u16, notify_addr: *mut u16) -> Self {
        let size_usize = size as usize;

        // Allocate descriptor table (16 bytes each, 16-byte aligned)
        let desc_layout =
            alloc::alloc::Layout::from_size_align(size_usize * size_of::<VringDesc>(), 16).unwrap();
        let desc = unsafe { alloc::alloc::alloc_zeroed(desc_layout) as *mut VringDesc };

        // Allocate available ring (4 bytes header + 2 bytes per entry, 2-byte aligned)
        let avail_size = 4 + size_usize * 2;
        let avail_layout = alloc::alloc::Layout::from_size_align(avail_size, 2).unwrap();
        let avail = unsafe { alloc::alloc::alloc_zeroed(avail_layout) as *mut VringAvail };

        // Allocate used ring (4 bytes header + 8 bytes per entry, 4-byte aligned)
        let used_size = 4 + size_usize * 8;
        let used_layout = alloc::alloc::Layout::from_size_align(used_size, 4).unwrap();
        let used = unsafe { alloc::alloc::alloc_zeroed(used_layout) as *mut VringUsed };

        // Chain free descriptors
        for i in 0..size {
            let d = unsafe { &mut *desc.add(i as usize) };
            d.next = if i + 1 < size { i + 1 } else { 0 };
        }

        Self {
            size,
            desc,
            avail,
            used,
            free_head: 0,
            last_used_idx: 0,
            notify_addr,
        }
    }

    /// Allocate a descriptor chain for a request.
    /// Returns the head descriptor index.
    fn alloc_desc(&mut self) -> u16 {
        let idx = self.free_head;
        let d = unsafe { &*self.desc.add(idx as usize) };
        self.free_head = d.next;
        idx
    }

    /// Free a descriptor back to the free list.
    fn free_desc(&mut self, idx: u16) {
        let d = unsafe { &mut *self.desc.add(idx as usize) };
        d.next = self.free_head;
        d.flags = 0;
        self.free_head = idx;
    }

    /// Submit a descriptor chain and notify the device.
    fn submit(&mut self, head: u16) {
        let avail = unsafe { &mut *self.avail };
        let idx = avail.idx;
        let ring_entry = unsafe {
            &mut *((self.avail as *mut u8).add(4 + (idx % self.size) as usize * 2) as *mut u16)
        };
        *ring_entry = head;

        fence(Ordering::Release);
        avail.idx = idx.wrapping_add(1);
        fence(Ordering::Release);

        // Notify device
        unsafe { core::ptr::write_volatile(self.notify_addr, 0) };
    }

    /// Poll for completion. Returns (descriptor head index, bytes written).
    fn poll_used(&mut self) -> (u32, u32) {
        loop {
            fence(Ordering::Acquire);
            let used = unsafe { &*self.used };
            if used.idx != self.last_used_idx {
                let elem = unsafe {
                    &*((self.used as *const u8)
                        .add(4 + (self.last_used_idx % self.size) as usize * 8)
                        as *const VringUsedElem)
                };
                self.last_used_idx = self.last_used_idx.wrapping_add(1);
                return (elem.id, elem.len);
            }
            core::hint::spin_loop();
        }
    }
}

/// A virtio PCI device with initialized transport.
pub struct VirtioDevice {
    regions: VirtioPciRegions,
    queues: alloc::vec::Vec<VirtQueue>,
}

impl VirtioDevice {
    /// Initialize a virtio device: reset, negotiate features, set up queues.
    ///
    /// PCI BARs must already be assigned and memory space / bus mastering enabled.
    pub fn new(ecam: &Ecam, pci_dev: &PciDevice, driver_features: u64) -> Option<Self> {
        let regions = find_virtio_pci_regions(ecam, pci_dev)?;
        log::debug!(
            "virtio: common={:#x} notify={:#x} device={:#x}",
            regions.common as u64,
            regions.notify as u64,
            regions.device as u64
        );
        let common = regions.common;

        // Reset device
        unsafe { mmio_write_u8(common, COMMON_DEVICE_STATUS, 0) };
        // Acknowledge
        unsafe { mmio_write_u8(common, COMMON_DEVICE_STATUS, VIRTIO_STATUS_ACKNOWLEDGE) };
        // Driver
        unsafe {
            mmio_write_u8(
                common,
                COMMON_DEVICE_STATUS,
                VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER,
            )
        };

        // Read device features
        unsafe { mmio_write_u32(common, COMMON_DEVICE_FEATURE_SELECT, 0) };
        let feat_lo = unsafe { mmio_read_u32(common, COMMON_DEVICE_FEATURE) } as u64;
        unsafe { mmio_write_u32(common, COMMON_DEVICE_FEATURE_SELECT, 1) };
        let feat_hi = unsafe { mmio_read_u32(common, COMMON_DEVICE_FEATURE) } as u64;
        let device_features = feat_lo | (feat_hi << 32);

        // Negotiate features (must include VIRTIO_F_VERSION_1 for modern)
        let features = (driver_features | VIRTIO_F_VERSION_1) & device_features;

        unsafe { mmio_write_u32(common, COMMON_GUEST_FEATURE_SELECT, 0) };
        unsafe { mmio_write_u32(common, COMMON_GUEST_FEATURE, features as u32) };
        unsafe { mmio_write_u32(common, COMMON_GUEST_FEATURE_SELECT, 1) };
        unsafe { mmio_write_u32(common, COMMON_GUEST_FEATURE, (features >> 32) as u32) };

        // Features OK
        unsafe {
            mmio_write_u8(
                common,
                COMMON_DEVICE_STATUS,
                VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_FEATURES_OK,
            )
        };
        let status = unsafe { mmio_read_u8(common, COMMON_DEVICE_STATUS) };
        if status & VIRTIO_STATUS_FEATURES_OK == 0 {
            log::debug!("virtio: features rejected (status={:#x})", status);
            return None;
        }
        log::debug!(
            "virtio: features OK, device={:#x}, negotiated={:#x}",
            device_features,
            features
        );

        // Set up queues
        let num_queues = unsafe { mmio_read_u16(common, COMMON_NUM_QUEUES) };
        let mut queues = alloc::vec::Vec::with_capacity(num_queues as usize);

        for i in 0..num_queues {
            unsafe { mmio_write_u16(common, COMMON_QUEUE_SELECT, i) };
            let queue_size = unsafe { mmio_read_u16(common, COMMON_QUEUE_SIZE) };
            if queue_size == 0 {
                continue;
            }

            let notify_off = unsafe { mmio_read_u16(common, COMMON_QUEUE_NOTIFY_OFF) };
            let notify_addr = unsafe {
                regions
                    .notify
                    .add(notify_off as usize * regions.notify_off_multiplier as usize)
            } as *mut u16;

            let vq = VirtQueue::new(queue_size, notify_addr);

            // Tell device about queue memory locations
            let desc_addr = vq.desc as u64;
            let avail_addr = vq.avail as u64;
            let used_addr = vq.used as u64;

            unsafe {
                mmio_write_u32(common, COMMON_QUEUE_DESC_LO, desc_addr as u32);
                mmio_write_u32(common, COMMON_QUEUE_DESC_HI, (desc_addr >> 32) as u32);
                mmio_write_u32(common, COMMON_QUEUE_AVAIL_LO, avail_addr as u32);
                mmio_write_u32(common, COMMON_QUEUE_AVAIL_HI, (avail_addr >> 32) as u32);
                mmio_write_u32(common, COMMON_QUEUE_USED_LO, used_addr as u32);
                mmio_write_u32(common, COMMON_QUEUE_USED_HI, (used_addr >> 32) as u32);

                // Disable MSI-X for this queue
                mmio_write_u16(common, COMMON_QUEUE_MSIX_VECTOR, 0xFFFF);

                // Enable queue
                mmio_write_u16(common, COMMON_QUEUE_ENABLE, 1);
            }

            queues.push(vq);
        }

        // Driver OK
        unsafe {
            mmio_write_u8(
                common,
                COMMON_DEVICE_STATUS,
                VIRTIO_STATUS_ACKNOWLEDGE
                    | VIRTIO_STATUS_DRIVER
                    | VIRTIO_STATUS_FEATURES_OK
                    | VIRTIO_STATUS_DRIVER_OK,
            )
        };

        Some(Self { regions, queues })
    }

    /// Get a mutable reference to a virtqueue.
    pub fn queue_mut(&mut self, index: usize) -> &mut VirtQueue {
        &mut self.queues[index]
    }

    /// Read from the device-specific config region.
    ///
    /// # Safety
    /// The offset and type must match the device's config layout.
    pub unsafe fn read_device_config<T: FromBytes>(&self, offset: usize) -> T {
        let ptr = unsafe { self.regions.device.add(offset) };
        let bytes = unsafe { core::slice::from_raw_parts(ptr, size_of::<T>()) };
        T::read_from_prefix(bytes).unwrap().0
    }
}

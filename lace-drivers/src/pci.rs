// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! PCI configuration space access and device enumeration
//!
//! Provides generic ECAM (Enhanced Configuration Access Mechanism) based PCI
//! config space access. x86 legacy I/O port access is in [`crate::x86::pci_legacy`].

/// PCI ECAM (Enhanced Configuration Access Mechanism) accessor.
///
/// ECAM maps PCI configuration space to a contiguous MMIO region where each
/// device function gets 4KB at `base + (bus << 20 | dev << 15 | func << 12)`.
pub struct Ecam {
    base: u64,
}

impl Ecam {
    /// Create a new ECAM accessor with the given MMIO base address.
    pub fn new(base: u64) -> Self {
        Self { base }
    }

    /// Calculate the MMIO address for a given BDF and register offset.
    fn addr(&self, bus: u8, dev: u8, func: u8, offset: u16) -> *mut u8 {
        let addr = self.base
            + ((bus as u64) << 20)
            + ((dev as u64) << 15)
            + ((func as u64) << 12)
            + (offset as u64);
        addr as *mut u8
    }

    pub fn read_u8(&self, bus: u8, dev: u8, func: u8, offset: u16) -> u8 {
        unsafe { core::ptr::read_volatile(self.addr(bus, dev, func, offset)) }
    }

    pub fn read_u16(&self, bus: u8, dev: u8, func: u8, offset: u16) -> u16 {
        unsafe { core::ptr::read_volatile(self.addr(bus, dev, func, offset) as *const u16) }
    }

    pub fn read_u32(&self, bus: u8, dev: u8, func: u8, offset: u16) -> u32 {
        unsafe { core::ptr::read_volatile(self.addr(bus, dev, func, offset) as *const u32) }
    }

    pub fn write_u8(&self, bus: u8, dev: u8, func: u8, offset: u16, value: u8) {
        unsafe { core::ptr::write_volatile(self.addr(bus, dev, func, offset), value) }
    }

    pub fn write_u16(&self, bus: u8, dev: u8, func: u8, offset: u16, value: u16) {
        unsafe { core::ptr::write_volatile(self.addr(bus, dev, func, offset) as *mut u16, value) }
    }

    pub fn write_u32(&self, bus: u8, dev: u8, func: u8, offset: u16, value: u32) {
        unsafe { core::ptr::write_volatile(self.addr(bus, dev, func, offset) as *mut u32, value) }
    }
}

/// A discovered PCI device.
#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
    pub bus: u8,
    pub dev: u8,
    pub func: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub header_type: u8,
}

/// Enumerate all PCI devices on a bus (single-segment, no bridge traversal).
pub fn enumerate_bus(ecam: &Ecam, bus: u8) -> alloc::vec::Vec<PciDevice> {
    let mut devices = alloc::vec::Vec::new();

    for dev in 0..32 {
        let vendor_id = ecam.read_u16(bus, dev, 0, 0x00);
        if vendor_id == 0xFFFF {
            continue;
        }

        let header_type = ecam.read_u8(bus, dev, 0, 0x0E);
        let max_func = if header_type & 0x80 != 0 { 8 } else { 1 };

        for func in 0..max_func {
            let vendor_id = ecam.read_u16(bus, dev, func, 0x00);
            if vendor_id == 0xFFFF {
                continue;
            }

            let device_id = ecam.read_u16(bus, dev, func, 0x02);
            let class_code = ecam.read_u8(bus, dev, func, 0x0B);
            let subclass = ecam.read_u8(bus, dev, func, 0x0A);
            let header_type = ecam.read_u8(bus, dev, func, 0x0E) & 0x7F;

            devices.push(PciDevice {
                bus,
                dev,
                func,
                vendor_id,
                device_id,
                class_code,
                subclass,
                header_type,
            });
        }
    }

    devices
}

/// Read a BAR (Base Address Register) value and return the MMIO base address.
/// Returns None for I/O BARs or unassigned BARs.
pub fn read_bar_mmio64(ecam: &Ecam, dev: &PciDevice, bar_index: u8) -> Option<u64> {
    let offset = 0x10 + (bar_index as u16) * 4;
    let low = ecam.read_u32(dev.bus, dev.dev, dev.func, offset);

    if low & 1 != 0 {
        return None;
    }

    let bar_type = (low >> 1) & 3;
    match bar_type {
        0 => {
            let addr = low & 0xFFFFFFF0;
            if addr == 0 { None } else { Some(addr as u64) }
        }
        2 => {
            let high = ecam.read_u32(dev.bus, dev.dev, dev.func, offset + 4);
            let addr = ((high as u64) << 32) | (low & 0xFFFFFFF0) as u64;
            if addr == 0 { None } else { Some(addr) }
        }
        _ => None,
    }
}

/// Simple PCI BAR allocator.
///
/// Assigns MMIO addresses to unassigned BARs from a given memory window.
/// Handles both 32-bit and 64-bit memory BARs. Skips I/O BARs.
pub struct BarAllocator {
    mmio_next: u64,
    mmio_end: u64,
}

impl BarAllocator {
    /// Create a new BAR allocator with the given MMIO window.
    pub fn new(mmio_base: u64, mmio_end: u64) -> Self {
        Self {
            mmio_next: mmio_base,
            mmio_end,
        }
    }

    /// Assign BARs for all devices on a bus.
    ///
    /// Also enables memory space access and bus mastering for each device.
    pub fn assign_bars(&mut self, ecam: &Ecam, devices: &[PciDevice]) {
        for dev in devices {
            let max_bars: u8 = if dev.header_type == 0 { 6 } else { 2 };
            let mut bar = 0u8;
            while bar < max_bars {
                let offset = 0x10 + (bar as u16) * 4;
                let orig = ecam.read_u32(dev.bus, dev.dev, dev.func, offset);

                if orig & 1 != 0 {
                    bar += 1;
                    continue;
                }

                let is_64bit = (orig >> 1) & 3 == 2;

                ecam.write_u32(dev.bus, dev.dev, dev.func, offset, 0xFFFFFFFF);
                if is_64bit {
                    ecam.write_u32(dev.bus, dev.dev, dev.func, offset + 4, 0xFFFFFFFF);
                }
                let size_low = ecam.read_u32(dev.bus, dev.dev, dev.func, offset);
                let size_high = if is_64bit {
                    ecam.read_u32(dev.bus, dev.dev, dev.func, offset + 4)
                } else {
                    0
                };

                ecam.write_u32(dev.bus, dev.dev, dev.func, offset, orig);
                if is_64bit {
                    let orig_high = ecam.read_u32(dev.bus, dev.dev, dev.func, offset + 4);
                    ecam.write_u32(dev.bus, dev.dev, dev.func, offset + 4, orig_high);
                }

                let mask = if is_64bit {
                    let combined = ((size_high as u64) << 32) | (size_low & 0xFFFFFFF0) as u64;
                    if combined == 0 {
                        bar += if is_64bit { 2 } else { 1 };
                        continue;
                    }
                    combined
                } else {
                    let masked = size_low & 0xFFFFFFF0;
                    if masked == 0 {
                        bar += 1;
                        continue;
                    }
                    masked as u64
                };
                let size = (!mask).wrapping_add(1);

                let aligned = (self.mmio_next + size - 1) & !(size - 1);
                if aligned + size > self.mmio_end {
                    bar += if is_64bit { 2 } else { 1 };
                    continue;
                }

                ecam.write_u32(dev.bus, dev.dev, dev.func, offset, aligned as u32);
                if is_64bit {
                    ecam.write_u32(
                        dev.bus,
                        dev.dev,
                        dev.func,
                        offset + 4,
                        (aligned >> 32) as u32,
                    );
                }
                self.mmio_next = aligned + size;

                bar += if is_64bit { 2 } else { 1 };
            }

            let cmd = ecam.read_u16(dev.bus, dev.dev, dev.func, 0x04);
            ecam.write_u16(dev.bus, dev.dev, dev.func, 0x04, cmd | 0x06);
        }
    }
}

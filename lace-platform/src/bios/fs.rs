// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
//! BIOS block device, disk I/O, and storage discovery.

use crate::bios;
use crate::bios::int::{BiosRegisters, bios_call};
use crate::fs::base::{BlockDevice, FsError};
use crate::fs::probe::DiscoveredStorage;
use alloc::boxed::Box;
use spin::Mutex;

// ---------------------------------------------------------------------------
// BIOS INT 13h disk I/O
// ---------------------------------------------------------------------------

/// INT 13h error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskError {
    Success,
    InvalidFunction,
    AddressMarkNotFound,
    WriteProtected,
    SectorNotFound,
    ResetFailed,
    DiskChanged,
    DriveNotReady,
    Timeout,
    ControllerFailure,
    Unknown(u8),
}

impl From<u8> for DiskError {
    fn from(code: u8) -> Self {
        match code {
            0x00 => DiskError::Success,
            0x01 => DiskError::InvalidFunction,
            0x02 => DiskError::AddressMarkNotFound,
            0x03 => DiskError::WriteProtected,
            0x04 => DiskError::SectorNotFound,
            0x05 => DiskError::ResetFailed,
            0x06 => DiskError::DiskChanged,
            0xAA => DiskError::DriveNotReady,
            0x80 => DiskError::Timeout,
            0x20 => DiskError::ControllerFailure,
            c => DiskError::Unknown(c),
        }
    }
}

/// Disk Address Packet (DAP) for INT 13h Extensions.
#[derive(Default, Debug, Clone, Copy)]
#[repr(C, packed)]
struct DiskAddressPacket {
    size: u8,
    unused: u8,
    count: u16,
    offset: u16,
    segment: u16,
    lba: u64,
}

/// Drive Parameters Packet for INT 13h Extensions AH=48h.
#[repr(C, packed)]
#[derive(Default, Debug, Clone, Copy)]
struct DriveParametersPacket {
    size: u16,
    flags: u16,
    cylinders: u32,
    heads: u32,
    sectors_per_track: u32,
    total_sectors: u64,
    bytes_per_sector: u16,
}

/// Check if BIOS INT 13h Extensions are present for a drive.
fn check_extensions_present(drive: u8) -> bool {
    let mut regs = BiosRegisters::new();
    regs.eax = 0x4100;
    regs.ebx = 0x55AA;
    regs.edx = drive as u32;

    unsafe { bios_call(0x13, &mut regs) };

    (regs.flags & 1) == 0 && (regs.ebx & 0xFFFF) == 0xAA55
}

/// Read sectors from disk using INT 13h Extensions.
unsafe fn int13h_read(drive: u8, lba: u64, count: u16, buffer: &mut [u8]) -> Result<(), DiskError> {
    let addr = buffer.as_mut_ptr() as u64;
    if addr >= 0x100000 {
        panic!("Buffer must be in low memory (<1MB)");
    }

    let segment = (addr >> 4) as u16;
    let offset = (addr & 0xF) as u16;

    let dap = DiskAddressPacket {
        size: 16,
        unused: 0,
        count,
        offset,
        segment,
        lba,
    };

    let mut regs = BiosRegisters::new();
    regs.eax = 0x4200;
    regs.edx = drive as u32;
    let dap_addr = &dap as *const _ as u32;
    regs.ds = (dap_addr >> 4) as u16;
    regs.esi = dap_addr & 0xF;

    unsafe { bios_call(0x13, &mut regs) };

    if (regs.flags & 1) != 0 {
        Err(DiskError::from((regs.eax >> 8) as u8))
    } else {
        Ok(())
    }
}

/// Get drive parameters using INT 13h Extensions AH=48h.
fn int13h_get_parameters(drive: u8) -> Result<DriveParametersPacket, DiskError> {
    let mut params = DriveParametersPacket {
        size: 26,
        ..Default::default()
    };

    let addr = &mut params as *mut _ as u64;
    if addr >= 0x100000 {
        panic!("Buffer must be in low memory (<1MB)");
    }

    let mut regs = BiosRegisters::new();
    regs.eax = 0x4800;
    regs.edx = drive as u32;
    regs.ds = (addr >> 4) as u16;
    regs.esi = (addr & 0xF) as u32;

    unsafe { bios_call(0x13, &mut regs) };

    if (regs.flags & 1) == 0 {
        Ok(params)
    } else {
        Err(DiskError::from((regs.eax >> 8) as u8))
    }
}

// ---------------------------------------------------------------------------
// Block device
// ---------------------------------------------------------------------------

/// 128KB bounce buffer in .bss (low memory, required by BIOS INT 13h).
static BOUNCE_BUFFER: Mutex<[u8; 131072]> = Mutex::new([0; 131072]);

/// BIOS block device implementing sector-level I/O.
///
/// Internally bounces reads through a static low-memory buffer as required
/// by BIOS INT 13h, then copies to the caller-provided buffer.
struct BiosBlockDevice {
    drive: u8,
    sector_size: u32,
    sector_count: u64,
}

impl BlockDevice for BiosBlockDevice {
    fn read_sectors(&mut self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), FsError> {
        let ss = self.sector_size as usize;
        let total_bytes = count as usize * ss;
        if buf.len() < total_bytes {
            return Err(FsError::Invalid);
        }

        let mut bounce = BOUNCE_BUFFER.lock();
        let max_sectors_per_chunk = bounce.len() / ss;
        let mut remaining = count as usize;
        let mut current_lba = lba;
        let mut buf_offset = 0;

        while remaining > 0 {
            let chunk = core::cmp::min(remaining, max_sectors_per_chunk);
            let chunk = core::cmp::min(chunk, 0xFFFF);

            let chunk_bytes = chunk * ss;
            unsafe {
                // SAFETY: The chunk fits the slice per the above calculations.
                int13h_read(
                    self.drive,
                    current_lba,
                    chunk as u16,
                    &mut bounce[..chunk_bytes],
                )
                .map_err(|e| FsError::Io(bios::Error::Disk(e)))?;
            }

            buf[buf_offset..buf_offset + chunk_bytes].copy_from_slice(&bounce[..chunk_bytes]);

            current_lba += chunk as u64;
            buf_offset += chunk_bytes;
            remaining -= chunk;
        }

        Ok(())
    }

    fn sector_size(&self) -> u32 {
        self.sector_size
    }

    fn sector_count(&self) -> u64 {
        self.sector_count
    }
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

fn probe_disk(drive: u8) -> Option<Box<dyn BlockDevice>> {
    if !check_extensions_present(drive) {
        return None;
    }
    let params = int13h_get_parameters(drive).ok()?;
    crate::debugln!("[BIOS] Discovered disk {:02X}", drive);
    Some(Box::new(BiosBlockDevice {
        drive,
        sector_size: params.bytes_per_sector as u32,
        sector_count: params.total_sectors,
    }))
}

/// Discover all BIOS disks.
pub fn discover_storage() -> DiscoveredStorage {
    let mut result = DiscoveredStorage::new();
    for id in 0x80..0xFF_u8 {
        if let Some(dev) = probe_disk(id) {
            result.disks.push(dev);
        }
    }
    result
}

/// Discover only the boot disk.
///
/// The boot drive number is stored at 0x7B00 by the Stage 1 bootloader
/// (passed in DL by the BIOS).
pub fn discover_boot_storage() -> DiscoveredStorage {
    // SAFETY: Address 0x7B00 is in the shared data area set up by Stage 1,
    // identity-mapped in the page tables, and valid for the lifetime of the
    // bootloader.
    let boot_drive: u8 = unsafe { *(0x7B00 as *const u8) };
    crate::debugln!("[BIOS] Boot drive: {:02X}", boot_drive);

    let mut result = DiscoveredStorage::new();
    result.disks = probe_disk(boot_drive).into_iter().collect();
    result
}

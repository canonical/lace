// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! QEMU fw_cfg device driver (DMA interface).
//!
//! The byte-by-byte port I/O transport is intentionally not supported: it
//! would only matter for QEMU versions predating 2.9, and we target modern
//! QEMU exclusively. [`FwCfg::probe`] fails with
//! [`FwCfgError::DmaUnsupported`] if the device is missing the DMA
//! interface.
//!
//! Currently only the x86 port I/O transport for the DMA register is
//! implemented; MMIO (arm64, riscv) can be added later alongside the port
//! constants.

use alloc::collections::BTreeMap;
use alloc::{string::String, vec, vec::Vec};
use lace_util::Display;
use zerocopy::byteorder::{BE, U16, U32, U64};
use zerocopy::{FromBytes, FromZeros, Immutable, IntoBytes, KnownLayout};

// fw_cfg selector keys
const FW_CFG_SIGNATURE: u16 = 0x0000;
const FW_CFG_ID: u16 = 0x0001;
const FW_CFG_FILE_DIR: u16 = 0x0019;

const FW_CFG_SIGNATURE_BYTES: [u8; 4] = *b"QEMU";
const FW_CFG_VERSION_DMA: u32 = 1 << 1;

// x86 I/O ports
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
const SELECTOR_PORT: u16 = 0x0510;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
const DATA_PORT: u16 = 0x0511;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
const DMA_ADDR_HIGH_PORT: u16 = 0x0514;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
const DMA_ADDR_LOW_PORT: u16 = 0x0518;

// DMA control bits (in the big-endian control word, low 16 bits).
const DMA_CTL_ERROR: u32 = 0x01;
const DMA_CTL_READ: u32 = 0x02;
// SKIP (0x04) and WRITE (0x10) are defined by the spec but unused here.
const DMA_CTL_SELECT: u32 = 0x08;

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
fn write_selector(key: u16) {
    unsafe { crate::x86::port_io::outw(SELECTOR_PORT, key) };
}

/// Byte-port read, used only by [`FwCfg::probe`] to read the signature and
/// feature-id before the DMA interface is confirmed.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
fn read_data_byte() -> u8 {
    unsafe { crate::x86::port_io::inb(DATA_PORT) }
}

/// Errors returned by the fw_cfg driver.
#[derive(Debug, Display)]
pub enum FwCfgProbeError {
    #[display("fw_cfg device not present")]
    NotPresent,
    #[display("fw_cfg DMA interface not supported (QEMU too old)")]
    DmaUnsupported,
}

/// Raw fw_cfg directory entry (all multi-byte fields are big-endian).
#[repr(C)]
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct FwCfgFile {
    size: U32<BE>,
    select: U16<BE>,
    _reserved: U16<BE>,
    name: [u8; 56],
}

impl FwCfgFile {
    /// Get the file size.
    pub fn size(&self) -> u32 {
        self.size.get()
    }

    /// Get the file selector.
    pub fn select(&self) -> u16 {
        self.select.get()
    }

    /// Get the file name.
    pub fn name(&self) -> &[u8] {
        trim_name(&self.name)
    }
}

/// Trim triling NUL bytes (if any) from a fw_cfg file name.
fn trim_name(name: &[u8]) -> &[u8] {
    let nul = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    &name[..nul]
}

/// DMA access struct shared with QEMU: all fields are big-endian.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout)]
struct DmaAccess {
    control: U32<BE>,
    length: U32<BE>,
    address: U64<BE>,
}

/// Handle to the QEMU fw_cfg device.
pub struct FwCfg {
    _private: (),
}

impl FwCfg {
    /// Probe for the fw_cfg device and its DMA interface.
    pub fn probe() -> Result<Self, FwCfgProbeError> {
        // Signature check via byte port I/O — the only byte-path reads we do.
        write_selector(FW_CFG_SIGNATURE);
        let sig = [
            read_data_byte(),
            read_data_byte(),
            read_data_byte(),
            read_data_byte(),
        ];
        if sig != FW_CFG_SIGNATURE_BYTES {
            return Err(FwCfgProbeError::NotPresent);
        }

        // FW_CFG_ID is a little-endian u32; bit 1 signals DMA support.
        write_selector(FW_CFG_ID);
        let id_bytes = [
            read_data_byte(),
            read_data_byte(),
            read_data_byte(),
            read_data_byte(),
        ];
        let id = u32::from_le_bytes(id_bytes);
        if id & FW_CFG_VERSION_DMA == 0 {
            return Err(FwCfgProbeError::DmaUnsupported);
        }

        Ok(Self { _private: () })
    }

    /// Issue a single DMA transaction against `buf` and spin until it
    /// completes. Assumes virtual == physical for the slice and the
    /// on-stack access struct, which holds across our bootblock, bios
    /// entry, and virt firmware (all identity-mapped during boot).
    fn dma(&self, control: u32, buf: &mut [u8]) {
        let mut access = DmaAccess {
            control: control.into(),
            length: (buf.len() as u32).into(),
            address: (buf.as_mut_ptr() as u64).into(),
        };
        let access_addr = &raw mut access as u64;

        #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
        unsafe {
            // 64-bit big-endian address register written as two 32-bit
            // big-endian halves. The write to the low half kicks off the
            // transfer.
            crate::x86::port_io::outl(DMA_ADDR_HIGH_PORT, ((access_addr >> 32) as u32).to_be());
            crate::x86::port_io::outl(DMA_ADDR_LOW_PORT, (access_addr as u32).to_be());
        }

        // Spin until QEMU clears the control word (or sets the error bit).
        loop {
            let ctl = unsafe { core::ptr::read_volatile(&raw const access.control) }.get();
            if ctl & DMA_CTL_ERROR != 0 {
                panic!("fw_cfg DMA transfer failed")
            }
            if ctl == 0 {
                break;
            }
            core::hint::spin_loop();
        }
    }

    /// Read the contents of the given key into `buf` in a single DMA
    /// transaction. Rewinds to offset 0 (via the `SELECT` control bit).
    pub fn read_key(&self, key: u16, buf: &mut [u8]) {
        let control = DMA_CTL_SELECT | DMA_CTL_READ | ((key as u32) << 16);
        self.dma(control, buf);
    }

    /// Continue reading from the currently selected key / file without
    /// rewinding. Must follow a prior [`read_key`](Self::read_key) /
    /// [`read_file`](Self::read_file) or another `read_continuation`.
    /// Lets callers stream a large file through a fixed-size buffer.
    pub fn read_continuation(&self, buf: &mut [u8]) {
        self.dma(DMA_CTL_READ, buf);
    }

    /// Find a file by name.
    pub fn find_file(&self, name: &[u8]) -> Option<FwCfgFile> {
        // Read the 4 byte big-endian entry count
        let mut count_buf = [0u8; 4];
        self.read_key(FW_CFG_FILE_DIR, &mut count_buf);
        let count = u32::from_be_bytes(count_buf) as usize;

        // Read file entries one-by-one
        let mut file = FwCfgFile::new_zeroed();
        for _ in 0..count {
            self.read_continuation(file.as_mut_bytes());
            if file.name() == name {
                return Some(file);
            }
        }
        None
    }

    /// Read a file into the provided buffer.
    pub fn read_file(&self, file: &FwCfgFile, buf: &mut [u8]) {
        self.read_key(file.select(), buf);
    }

    /// Read a file into a new Vec of its full size.
    pub fn read_file_to_vec(&self, file: &FwCfgFile) -> Vec<u8> {
        let mut buf = vec![0u8; file.size() as usize];
        self.read_file(file, &mut buf);
        buf
    }

    /// Process the ACPI table-loader (`etc/table-loader`) to load and link
    /// ACPI tables into memory.
    ///
    /// Each table is allocated via the caller-provided `alloc_table`
    /// closure, which takes `(size, align)` and returns a slice the
    /// driver writes the table data into. The caller is responsible for
    /// placing these allocations in memory that will be preserved across
    /// OS handoff (e.g. firmware-managed `AcpiNvs` pages); the driver
    /// never touches the system heap.
    ///
    /// Returns the RSDP slice so the caller can record its physical
    /// address, or `None` if no table-loader file is present.
    pub fn load_acpi_tables(
        &self,
        mut alloc_table: impl FnMut(usize, usize) -> &'static mut [u8],
    ) -> Option<&'static mut [u8]> {
        const LOADER_ENTRY_SIZE: usize = 128;
        const LOADER_CMD_ALLOCATE: u32 = 0x1;
        const LOADER_CMD_ADD_POINTER: u32 = 0x2;
        const LOADER_CMD_ADD_CHECKSUM: u32 = 0x3;
        const LOADER_CMD_WRITE_POINTER: u32 = 0x4;

        let loader_file = self.find_file(b"etc/table-loader")?;
        let loader_data = self.read_file_to_vec(&loader_file);

        let mut allocations: BTreeMap<&[u8], &'static mut [u8]> = BTreeMap::new();

        for entry_bytes in loader_data.chunks_exact(LOADER_ENTRY_SIZE) {
            let command = u32::from_le_bytes(entry_bytes[0..4].try_into().unwrap());
            let payload = &entry_bytes[4..];

            match command {
                LOADER_CMD_ALLOCATE => {
                    let name = trim_name(&payload[..56]);
                    let align = u32::from_le_bytes(payload[56..60].try_into().unwrap());
                    let align = align.max(1) as usize;

                    // Look up the file size without alloc, then let the
                    // caller provide the backing memory with the right
                    // memory type / alignment, and stream the file data
                    // into it directly.
                    let file = self.find_file(name).unwrap();
                    let size = file.size() as usize;
                    let region = alloc_table(size, align);
                    self.read_file(&file, region);
                    log::debug!(
                        "table-loader: allocated {} at {:p}",
                        String::from_utf8_lossy(name),
                        region.as_ptr()
                    );
                    allocations.insert(name, region);
                }
                LOADER_CMD_ADD_POINTER => {
                    let dest_file = trim_name(&payload[..56]);
                    let src_file = trim_name(&payload[56..112]);
                    let offset = u32::from_le_bytes(payload[112..116].try_into().unwrap()) as usize;
                    let size = payload[116] as usize;

                    let src_addr = allocations
                        .get(src_file)
                        .unwrap_or_else(|| {
                            panic!(
                                "table-loader: src not found: {}",
                                String::from_utf8_lossy(src_file)
                            )
                        })
                        .as_ptr() as u64;
                    let dest = allocations.get_mut(dest_file).unwrap_or_else(|| {
                        panic!(
                            "table-loader: dest not found: {}",
                            String::from_utf8_lossy(dest_file)
                        )
                    });

                    // Add src base address to the value at dest[offset..offset+size]
                    match size {
                        1 => {
                            let v = dest[offset];
                            dest[offset] = v.wrapping_add(src_addr as u8);
                        }
                        2 => {
                            let v =
                                u16::from_le_bytes(dest[offset..offset + 2].try_into().unwrap());
                            dest[offset..offset + 2]
                                .copy_from_slice(&v.wrapping_add(src_addr as u16).to_le_bytes());
                        }
                        4 => {
                            let v =
                                u32::from_le_bytes(dest[offset..offset + 4].try_into().unwrap());
                            dest[offset..offset + 4]
                                .copy_from_slice(&v.wrapping_add(src_addr as u32).to_le_bytes());
                        }
                        8 => {
                            let v =
                                u64::from_le_bytes(dest[offset..offset + 8].try_into().unwrap());
                            dest[offset..offset + 8]
                                .copy_from_slice(&v.wrapping_add(src_addr).to_le_bytes());
                        }
                        _ => panic!("table-loader: unsupported pointer size: {}", size),
                    }
                }
                LOADER_CMD_ADD_CHECKSUM => {
                    let file = trim_name(&payload[..56]);
                    let cksum_offset =
                        u32::from_le_bytes(payload[56..60].try_into().unwrap()) as usize;
                    let start = u32::from_le_bytes(payload[60..64].try_into().unwrap()) as usize;
                    let length = u32::from_le_bytes(payload[64..68].try_into().unwrap()) as usize;

                    let region = allocations.get_mut(file).unwrap_or_else(|| {
                        panic!(
                            "table-loader: file not found: {}",
                            String::from_utf8_lossy(file)
                        )
                    });

                    // Zero the checksum byte first
                    region[cksum_offset] = 0;
                    // Sum all bytes in the range
                    let sum: u8 = region[start..start + length]
                        .iter()
                        .fold(0u8, |acc, &b| acc.wrapping_add(b));
                    // Store negated sum so total sums to zero
                    region[cksum_offset] = 0u8.wrapping_sub(sum);
                }
                LOADER_CMD_WRITE_POINTER => {
                    // Write pointer back to fw_cfg — not needed for our use case
                }
                _ => {} // Skip unknown commands
            }
        }

        allocations.remove(b"etc/acpi/rsdp".as_ref())
    }
}

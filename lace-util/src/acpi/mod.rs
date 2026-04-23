// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! Minimal ACPI table parser
//!
//! Parses RSDP, XSDT/RSDT, and locates tables by signature.

use zerocopy::{
    FromBytes, Immutable, KnownLayout,
    byteorder::{LE, U32, U64},
};

pub mod fadt;
pub mod mcfg;

/// ACPI RSDP v1 (revision 0), 20 bytes.
#[repr(C, packed)]
#[derive(FromBytes, Immutable, KnownLayout)]
pub struct RsdpV1 {
    pub signature: [u8; 8],
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub revision: u8,
    pub rsdt_address: U32<LE>,
}

/// ACPI RSDP v2 extension (revision >= 2), 36 bytes total.
#[repr(C, packed)]
#[derive(FromBytes, Immutable, KnownLayout)]
pub struct RsdpV2 {
    pub v1: RsdpV1,
    pub length: U32<LE>,
    pub xsdt_address: U64<LE>,
    pub extended_checksum: u8,
    pub _reserved: [u8; 3],
}

/// ACPI System Description Table header (common to all ACPI tables).
#[repr(C, packed)]
#[derive(FromBytes, Immutable, KnownLayout)]
pub struct SdtHeader {
    pub signature: [u8; 4],
    pub length: U32<LE>,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: U32<LE>,
    pub creator_id: U32<LE>,
    pub creator_revision: U32<LE>,
}

/// Parsed RSDP — provides access to either RSDT (v1) or XSDT (v2) address.
pub struct Rsdp {
    pub revision: u8,
    pub rsdt_address: u32,
    pub xsdt_address: u64, // 0 if revision < 2
}

/// Sum of `data` as a single wrapping byte. ACPI mandates that the
/// checksummed region sums to zero.
fn acpi_checksum(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b))
}

impl Rsdp {
    /// Parse the RSDP from a byte slice.
    pub fn parse(data: &[u8]) -> Option<Self> {
        let (v1, _) = RsdpV1::ref_from_prefix(data).ok()?;
        if &v1.signature != b"RSD PTR " {
            return None;
        }

        // ACPI v1 checksum: first 20 bytes sum to zero.
        if acpi_checksum(&data[..size_of::<RsdpV1>()]) != 0 {
            return None;
        }

        let revision = v1.revision;
        let rsdt_address = v1.rsdt_address.get();

        let xsdt_address = if revision >= 2 {
            let (v2, _) = RsdpV2::ref_from_prefix(data).ok()?;
            // ACPI v2+ extended checksum: the first `length` bytes sum to zero.
            let length = v2.length.get().try_into().unwrap();
            if length < size_of::<RsdpV2>() || length > data.len() {
                return None;
            }
            if acpi_checksum(&data[..length]) != 0 {
                return None;
            }
            v2.xsdt_address.get()
        } else {
            0
        };

        Some(Self {
            revision,
            rsdt_address,
            xsdt_address,
        })
    }

    /// Find a table by signature in the XSDT or RSDT.
    ///
    /// `deref` resolves a physical address and length to a byte slice.
    /// This is the only part that requires unsafe on bare metal (the caller
    /// provides it), keeping the search logic itself safe and testable.
    pub fn find_table<'a>(
        &self,
        signature: &[u8; 4],
        deref: impl Fn(u64, usize) -> &'a [u8],
    ) -> Option<u64> {
        let (sdt_addr, ptr_size) = if self.revision >= 2 && self.xsdt_address != 0 {
            (self.xsdt_address, 8)
        } else {
            (self.rsdt_address.into(), 4)
        };

        // Read the SDT header to get the total length
        let header_bytes = deref(sdt_addr, size_of::<SdtHeader>());
        let (header, _) = SdtHeader::ref_from_prefix(header_bytes).ok()?;
        let total_length = header.length.get().try_into().unwrap();

        // Read the full SDT
        let sdt_data = deref(sdt_addr, total_length);

        // Walk entries
        let entries_offset = size_of::<SdtHeader>();
        let entries_len = total_length.checked_sub(entries_offset)?;
        let num_entries = entries_len / ptr_size;

        for i in 0..num_entries {
            let off = entries_offset + i * ptr_size;
            if off + ptr_size > sdt_data.len() {
                break;
            }
            let addr = if ptr_size == 8 {
                u64::from_le_bytes(sdt_data[off..off + 8].try_into().ok()?)
            } else {
                u32::from_le_bytes(sdt_data[off..off + 4].try_into().ok()?).into()
            };

            // Read the table's signature
            let table_header = deref(addr, 4);
            if table_header == signature {
                return Some(addr);
            }
        }
        None
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// Write an ACPI-style checksum byte at `data[checksum_index]` so the
    /// covered region sums to zero.
    fn fix_checksum(data: &mut [u8], checksum_index: usize, length: usize) {
        data[checksum_index] = 0;
        let sum = acpi_checksum(&data[..length]);
        data[checksum_index] = 0u8.wrapping_sub(sum);
    }

    #[test]
    fn test_parse_rsdp_v1() {
        let mut data = [0u8; 20];
        data[0..8].copy_from_slice(b"RSD PTR ");
        data[15] = 0; // revision 0
        data[16..20].copy_from_slice(&0x12345678u32.to_le_bytes());
        fix_checksum(&mut data, 8, 20);

        let rsdp = Rsdp::parse(&data).unwrap();
        assert_eq!(rsdp.revision, 0);
        assert_eq!(rsdp.rsdt_address, 0x12345678);
        assert_eq!(rsdp.xsdt_address, 0);
    }

    #[test]
    fn test_parse_rsdp_v2() {
        let mut data = [0u8; 36];
        data[0..8].copy_from_slice(b"RSD PTR ");
        data[15] = 2; // revision 2
        data[16..20].copy_from_slice(&0xAABBCCDDu32.to_le_bytes());
        data[20..24].copy_from_slice(&36u32.to_le_bytes());
        data[24..32].copy_from_slice(&0xDEADBEEF_CAFEBABEu64.to_le_bytes());
        fix_checksum(&mut data, 8, 20);
        fix_checksum(&mut data, 32, 36);

        let rsdp = Rsdp::parse(&data).unwrap();
        assert_eq!(rsdp.revision, 2);
        assert_eq!(rsdp.rsdt_address, 0xAABBCCDD);
        assert_eq!(rsdp.xsdt_address, 0xDEADBEEF_CAFEBABE);
    }

    #[test]
    fn test_parse_rsdp_bad_signature() {
        let mut data = [0u8; 20];
        data[0..8].copy_from_slice(b"NOT RSDP");
        assert!(Rsdp::parse(&data).is_none());
    }

    #[test]
    fn test_parse_rsdp_too_small() {
        assert!(Rsdp::parse(&[0u8; 10]).is_none());
    }

    #[test]
    fn test_parse_rsdp_bad_checksum_v1() {
        let mut data = [0u8; 20];
        data[0..8].copy_from_slice(b"RSD PTR ");
        data[15] = 0;
        data[16..20].copy_from_slice(&0x12345678u32.to_le_bytes());
        // Deliberately do not fix the checksum byte.
        assert!(Rsdp::parse(&data).is_none());
    }

    #[test]
    fn test_parse_rsdp_bad_checksum_v2() {
        let mut data = [0u8; 36];
        data[0..8].copy_from_slice(b"RSD PTR ");
        data[15] = 2;
        data[20..24].copy_from_slice(&36u32.to_le_bytes());
        fix_checksum(&mut data, 8, 20);
        // Leave the extended checksum wrong.
        assert!(Rsdp::parse(&data).is_none());
    }

    #[test]
    fn test_find_table_xsdt() {
        let sdt_hdr_size = size_of::<SdtHeader>();

        // Place the XSDT at a non-zero offset so the revision>=2 &&
        // xsdt_address!=0 branch is taken (rather than falling back to RSDT).
        let xsdt_base = 0x100usize;
        let xsdt_entries = 2;
        let xsdt_len = sdt_hdr_size + xsdt_entries * 8;
        let facp_off = xsdt_base + xsdt_len;
        let mcfg_off = facp_off + sdt_hdr_size;
        let total = mcfg_off + sdt_hdr_size;

        let mut buf = alloc::vec![0u8; total];

        // XSDT header
        buf[xsdt_base..xsdt_base + 4].copy_from_slice(b"XSDT");
        buf[xsdt_base + 4..xsdt_base + 8].copy_from_slice(&(xsdt_len as u32).to_le_bytes());

        // FACP and MCFG table headers
        buf[facp_off..facp_off + 4].copy_from_slice(b"FACP");
        buf[mcfg_off..mcfg_off + 4].copy_from_slice(b"MCFG");

        // 8-byte XSDT entries
        let e0 = xsdt_base + sdt_hdr_size;
        buf[e0..e0 + 8].copy_from_slice(&(facp_off as u64).to_le_bytes());
        buf[e0 + 8..e0 + 16].copy_from_slice(&(mcfg_off as u64).to_le_bytes());

        let rsdp = Rsdp {
            revision: 2,
            rsdt_address: 0,
            xsdt_address: xsdt_base as u64,
        };

        // deref treats addresses as offsets into buf
        let deref = |addr: u64, len: usize| &buf[addr as usize..addr as usize + len];

        assert_eq!(rsdp.find_table(b"MCFG", &deref), Some(mcfg_off as u64));
        assert_eq!(rsdp.find_table(b"FACP", &deref), Some(facp_off as u64));
        assert_eq!(rsdp.find_table(b"HPET", &deref), None);
    }

    #[test]
    fn test_find_table_rsdt() {
        let sdt_hdr_size = size_of::<SdtHeader>();

        // Build RSDT with 2 entries (32-bit pointers)
        let rsdt_entries = 2;
        let rsdt_len = sdt_hdr_size + rsdt_entries * 4;
        let facp_off = rsdt_len;
        let apic_off = facp_off + sdt_hdr_size;
        let total = apic_off + sdt_hdr_size;

        let mut buf = alloc::vec![0u8; total];

        buf[0..4].copy_from_slice(b"RSDT");
        buf[4..8].copy_from_slice(&(rsdt_len as u32).to_le_bytes());

        buf[facp_off..facp_off + 4].copy_from_slice(b"FACP");
        buf[apic_off..apic_off + 4].copy_from_slice(b"APIC");

        buf[sdt_hdr_size..sdt_hdr_size + 4].copy_from_slice(&(facp_off as u32).to_le_bytes());
        buf[sdt_hdr_size + 4..sdt_hdr_size + 8].copy_from_slice(&(apic_off as u32).to_le_bytes());

        let rsdp = Rsdp {
            revision: 0,
            rsdt_address: 0,
            xsdt_address: 0,
        };

        let deref = |addr: u64, len: usize| &buf[addr as usize..addr as usize + len];

        assert_eq!(rsdp.find_table(b"APIC", &deref), Some(apic_off as u64));
        assert_eq!(rsdp.find_table(b"FACP", &deref), Some(facp_off as u64));
        assert_eq!(rsdp.find_table(b"MCFG", &deref), None);
    }
}

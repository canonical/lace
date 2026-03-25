// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.

//! Portable MBR (Master Boot Record) partition table parser.
//!
//! Operates on any [`BlockDevice`] and supports arbitrary sector sizes.
//! Handles both primary partitions and extended partitions with logical
//! partitions inside them (EBR chain).

use alloc::vec;
use alloc::vec::Vec;
use zerocopy::byteorder::{LE, U16, U32};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use super::base::{BlockDevice, FsError};

/// MBR partition table entry (16 bytes), per IBM PC specification.
#[repr(C, packed)]
#[derive(Clone, Copy, FromBytes, Immutable, IntoBytes, KnownLayout)]
struct MbrPartitionEntry {
    status: u8,
    chs_first: [u8; 3],
    partition_type: u8,
    chs_last: [u8; 3],
    lba_start: U32<LE>,
    lba_size: U32<LE>,
}

/// MBR boot sector (512 bytes at LBA 0).
#[repr(C)]
#[derive(FromBytes, Immutable, KnownLayout)]
struct MbrSector {
    _bootstrap: [u8; 440],
    _disk_id: U32<LE>,
    _reserved: U16<LE>,
    partitions: [MbrPartitionEntry; 4],
    signature: [u8; 2],
}

/// A discovered MBR partition.
#[derive(Debug, Clone, Copy)]
pub struct MbrPartition {
    pub start_lba: u64,
    pub size_lba: u64,
    pub partition_type: u8,
}

/// Extended partition type IDs.
fn is_extended_type(t: u8) -> bool {
    matches!(t, 0x05 | 0x0F | 0x85)
}

/// Read and validate an MBR sector at the given byte offset.
fn read_mbr_sector(dev: &mut dyn BlockDevice, byte_offset: u64) -> Result<MbrSector, FsError> {
    let mut buf = vec![0u8; size_of::<MbrSector>()];
    dev.read_bytes(byte_offset, &mut buf)?;

    let (sector, _) = MbrSector::read_from_prefix(&buf).map_err(|_| FsError::Invalid)?;

    if sector.signature != [0x55, 0xAA] {
        return Err(FsError::NotFound);
    }

    Ok(sector)
}

/// Parse the MBR partition table from a block device.
///
/// Returns all primary and logical partitions. Extended partitions are
/// followed (EBR chain) to discover logical partitions within them.
pub fn parse_mbr(dev: &mut dyn BlockDevice) -> Result<Vec<MbrPartition>, FsError> {
    let mbr = read_mbr_sector(dev, 0)?;

    let mut partitions = Vec::new();

    for entry in &mbr.partitions {
        let ptype = entry.partition_type;
        let start = entry.lba_start.get() as u64;
        let size = entry.lba_size.get() as u64;

        // Skip empty entries
        if ptype == 0x00 || size == 0 {
            continue;
        }

        if is_extended_type(ptype) {
            // Follow EBR chain for logical partitions
            parse_ebr_chain(dev, start, start, &mut partitions)?;
        } else {
            partitions.push(MbrPartition {
                start_lba: start,
                size_lba: size,
                partition_type: ptype,
            });
        }
    }

    if partitions.is_empty() {
        return Err(FsError::NotFound);
    }

    Ok(partitions)
}

/// Walk the EBR (Extended Boot Record) chain starting at `ebr_lba`.
///
/// `extended_start` is the LBA of the outer extended partition (used to
/// resolve relative offsets in chained EBR entries).
fn parse_ebr_chain(
    dev: &mut dyn BlockDevice,
    ebr_lba: u64,
    extended_start: u64,
    partitions: &mut Vec<MbrPartition>,
) -> Result<(), FsError> {
    let mut current_lba = ebr_lba;

    // Guard against infinite loops in malformed EBR chains
    let max_logical = 256;
    let mut count = 0;

    loop {
        if count >= max_logical {
            break;
        }
        count += 1;

        let sector_size = dev.sector_size() as u64;
        let ebr = match read_mbr_sector(dev, current_lba * sector_size) {
            Ok(s) => s,
            Err(FsError::NotFound) => break,
            Err(e) => return Err(e),
        };

        // First entry: the logical partition (relative to this EBR)
        let logical = &ebr.partitions[0];
        if logical.partition_type != 0x00 && logical.lba_size.get() > 0 {
            partitions.push(MbrPartition {
                start_lba: current_lba + logical.lba_start.get() as u64,
                size_lba: logical.lba_size.get() as u64,
                partition_type: logical.partition_type,
            });
        }

        // Second entry: link to next EBR (relative to extended partition start)
        let next = &ebr.partitions[1];
        if is_extended_type(next.partition_type) && next.lba_size.get() > 0 {
            current_lba = extended_start + next.lba_start.get() as u64;
        } else {
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::fs::base::FsError;
    use crate::fs::testutil::MemDisk;

    fn write_mbr_signature(disk: &mut MemDisk, sector_byte_offset: usize) {
        disk.data[sector_byte_offset + 510] = 0x55;
        disk.data[sector_byte_offset + 511] = 0xAA;
    }

    fn write_partition_entry(
        disk: &mut MemDisk,
        sector_byte_offset: usize,
        slot: usize,
        ptype: u8,
        lba_start: u32,
        lba_size: u32,
    ) {
        let entry = MbrPartitionEntry {
            status: 0,
            chs_first: [0; 3],
            partition_type: ptype,
            chs_last: [0; 3],
            lba_start: U32::new(lba_start),
            lba_size: U32::new(lba_size),
        };
        let entry_bytes: &[u8] = zerocopy::IntoBytes::as_bytes(&entry);
        let offset = sector_byte_offset + 446 + slot * size_of::<MbrPartitionEntry>();
        disk.write_at(offset, entry_bytes);
    }

    #[test]
    fn test_parse_mbr_two_primary_partitions() {
        let mut disk = MemDisk::new(512, 20480);
        write_mbr_signature(&mut disk, 0);
        write_partition_entry(&mut disk, 0, 0, 0x83, 2048, 4096);
        write_partition_entry(&mut disk, 0, 1, 0x82, 6144, 8192);

        let parts = parse_mbr(&mut disk).unwrap();
        assert_eq!(parts.len(), 2);

        assert_eq!(parts[0].start_lba, 2048);
        assert_eq!(parts[0].size_lba, 4096);
        assert_eq!(parts[0].partition_type, 0x83);

        assert_eq!(parts[1].start_lba, 6144);
        assert_eq!(parts[1].size_lba, 8192);
        assert_eq!(parts[1].partition_type, 0x82);
    }

    #[test]
    fn test_parse_mbr_skips_empty_entries() {
        let mut disk = MemDisk::new(512, 20480);
        write_mbr_signature(&mut disk, 0);
        // Slot 0: empty (type 0x00)
        write_partition_entry(&mut disk, 0, 1, 0x83, 2048, 4096);
        // Slot 2: empty
        write_partition_entry(&mut disk, 0, 3, 0x83, 8192, 2048);

        let parts = parse_mbr(&mut disk).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].start_lba, 2048);
        assert_eq!(parts[1].start_lba, 8192);
    }

    #[test]
    fn test_parse_mbr_extended_with_two_logical() {
        let mut disk = MemDisk::new(512, 40960);
        write_mbr_signature(&mut disk, 0);

        // Primary partition in slot 0
        write_partition_entry(&mut disk, 0, 0, 0x83, 2048, 4096);
        // Extended partition in slot 1 starting at LBA 8192
        write_partition_entry(&mut disk, 0, 1, 0x05, 8192, 16384);

        // EBR 1 at LBA 8192: logical partition at offset 1, size 4096
        let ebr1_offset = 8192 * 512;
        write_mbr_signature(&mut disk, ebr1_offset);
        write_partition_entry(&mut disk, ebr1_offset, 0, 0x83, 1, 4096);
        // Link to next EBR at offset 4097 from extended start (8192)
        write_partition_entry(&mut disk, ebr1_offset, 1, 0x05, 4097, 8192);

        // EBR 2 at LBA 8192 + 4097 = 12289: logical partition at offset 1, size 8191
        let ebr2_offset = 12289 * 512;
        write_mbr_signature(&mut disk, ebr2_offset);
        write_partition_entry(&mut disk, ebr2_offset, 0, 0x83, 1, 8191);
        // No next link (end of chain)

        let parts = parse_mbr(&mut disk).unwrap();
        assert_eq!(parts.len(), 3, "1 primary + 2 logical");

        // Primary
        assert_eq!(parts[0].start_lba, 2048);
        assert_eq!(parts[0].size_lba, 4096);

        // Logical 1: EBR at 8192, partition at offset 1 → LBA 8193
        assert_eq!(parts[1].start_lba, 8193);
        assert_eq!(parts[1].size_lba, 4096);

        // Logical 2: EBR at 12289, partition at offset 1 → LBA 12290
        assert_eq!(parts[2].start_lba, 12290);
        assert_eq!(parts[2].size_lba, 8191);
    }

    #[test]
    fn test_parse_mbr_bad_signature() {
        let mut disk = MemDisk::new(512, 128);
        // No 0x55AA signature — all zeros

        let result = parse_mbr(&mut disk);
        assert!(matches!(result, Err(FsError::NotFound)));
    }

    #[test]
    fn test_parse_mbr_no_partitions() {
        let mut disk = MemDisk::new(512, 128);
        write_mbr_signature(&mut disk, 0);
        // Valid signature but all partition entries are zero

        let result = parse_mbr(&mut disk);
        assert!(matches!(result, Err(FsError::NotFound)));
    }

    #[test]
    fn test_parse_mbr_four_primary_partitions() {
        let mut disk = MemDisk::new(512, 40960);
        write_mbr_signature(&mut disk, 0);
        write_partition_entry(&mut disk, 0, 0, 0x83, 2048, 2048);
        write_partition_entry(&mut disk, 0, 1, 0x83, 4096, 2048);
        write_partition_entry(&mut disk, 0, 2, 0x82, 6144, 2048);
        write_partition_entry(&mut disk, 0, 3, 0x0C, 8192, 2048); // FAT32 LBA

        let parts = parse_mbr(&mut disk).unwrap();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[3].partition_type, 0x0C);
    }
}

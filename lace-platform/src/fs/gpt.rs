// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! Portable GPT (GUID Partition Table) parser.
//!
//! Operates on any [`BlockDevice`] and supports arbitrary sector sizes.

use alloc::vec;
use alloc::vec::Vec;
use lace_util::{Guid, OrderedGuid};
use zerocopy::byteorder::{LE, U16, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use super::base::{BlockDevice, FsError};

/// GPT header, per UEFI specification.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout)]
struct GptHeader {
    signature: [u8; 8],
    revision: U32<LE>,
    header_size: U32<LE>,
    crc32: U32<LE>,
    reserved: U32<LE>,
    current_lba: U64<LE>,
    backup_lba: U64<LE>,
    first_usable_lba: U64<LE>,
    last_usable_lba: U64<LE>,
    disk_guid: OrderedGuid<LE>,
    partition_entry_lba: U64<LE>,
    num_partition_entries: U32<LE>,
    partition_entry_size: U32<LE>,
    partition_entry_crc32: U32<LE>,
    _padding: U32<LE>,
}

/// GPT partition entry, per UEFI specification.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout)]
struct GptPartitionEntry {
    type_guid: OrderedGuid<LE>,
    unique_guid: OrderedGuid<LE>,
    first_lba: U64<LE>,
    last_lba: U64<LE>,
    attributes: U64<LE>,
    name: [U16<LE>; 36],
}

/// A discovered GPT partition.
#[derive(Debug, Clone, Copy)]
pub struct GptPartition {
    pub start_lba: u64,
    pub size_lba: u64,
    pub type_guid: Guid,
    pub unique_guid: Guid,
}

/// Parse the GPT partition table from a block device.
///
/// `sector_size` is required because the GPT header resides at LBA 1 (i.e.
/// byte offset `sector_size`), and partition entries are also addressed by LBA.
pub fn parse_gpt(
    dev: &mut dyn BlockDevice,
    sector_size: u32,
) -> Result<Vec<GptPartition>, FsError> {
    let ss = sector_size as u64;

    // Read GPT header at LBA 1
    let mut header_buf = vec![0u8; size_of::<GptHeader>()];
    dev.read_bytes(ss, &mut header_buf)?;

    let (header, _) = GptHeader::read_from_prefix(&header_buf).map_err(|_| FsError::Invalid)?;

    if &header.signature != b"EFI PART" {
        return Err(FsError::NotFound);
    }

    let num_entries = header.num_partition_entries.get();
    let entry_size = header.partition_entry_size.get() as u64;
    let entry_start_byte = header
        .partition_entry_lba
        .get()
        .checked_mul(ss)
        .ok_or(FsError::Invalid)?;

    if entry_size < size_of::<GptPartitionEntry>() as u64 {
        return Err(FsError::Invalid);
    }

    // Validate total entry array size does not overflow
    (num_entries as u64)
        .checked_mul(entry_size)
        .and_then(|total| entry_start_byte.checked_add(total))
        .ok_or(FsError::Invalid)?;

    let mut partitions = Vec::new();

    for i in 0..num_entries {
        let entry_offset = entry_start_byte + (i as u64) * entry_size;
        let mut entry_buf = vec![0u8; size_of::<GptPartitionEntry>()];
        dev.read_bytes(entry_offset, &mut entry_buf)?;

        let (entry, _) =
            GptPartitionEntry::read_from_prefix(&entry_buf).map_err(|_| FsError::Invalid)?;

        // Skip unused entries (type GUID is all zeros)
        if entry.type_guid == OrderedGuid::<LE>::default() {
            continue;
        }

        let first_lba = entry.first_lba.get();
        let last_lba = entry.last_lba.get();
        let size_lba = last_lba
            .checked_sub(first_lba)
            .and_then(|d| d.checked_add(1))
            .ok_or(FsError::Invalid)?;

        partitions.push(GptPartition {
            start_lba: first_lba,
            size_lba,
            type_guid: entry.type_guid.into(),
            unique_guid: entry.unique_guid.into(),
        });
    }

    Ok(partitions)
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use crate::fs::base::FsError;
    use crate::fs::testutil::MemDisk;
    use lace_util::count_blocks_aligned_up;

    /// Build a disk image with a GPT partition table containing the given
    /// partition data blobs.
    ///
    /// Computes partition layout from the data sizes, delegates header and
    /// entry writing to [`build_gpt_disk`], then writes the data bytes.
    /// Returns a [`MemDisk`] with 512-byte sectors.
    pub(crate) fn build_gpt_disk_with_data(partitions: &[&[u8]]) -> MemDisk {
        let sector_size: u32 = 512;
        let ss = sector_size as usize;

        let entry_sectors =
            count_blocks_aligned_up!(partitions.len() * size_of::<GptPartitionEntry>(), ss);
        let first_data_lba = (2 + entry_sectors) as u64;

        // A non-zero type GUID so entries are recognised as used
        let type_guid: Guid = Guid::read_from_bytes(&[
            0xAF, 0x3D, 0xC6, 0x0F, 0x83, 0x84, 0x72, 0x47, 0x8E, 0x79, 0x3D, 0x69, 0xD8, 0x47,
            0x7D, 0xE4,
        ])
        .unwrap();

        let mut next_lba = first_data_lba;
        let mut part_specs: Vec<(u64, u64, Guid)> = Vec::new();
        for data in partitions {
            assert!(!data.is_empty(), "partition data must not be empty");
            let data_sectors = count_blocks_aligned_up!(data.len() as u64, sector_size as u64);
            part_specs.push((next_lba, next_lba + data_sectors - 1, type_guid));
            next_lba += data_sectors;
        }

        let mut disk = build_gpt_disk(sector_size, &part_specs);

        // Ensure the disk is large enough to hold the partition data
        let needed = next_lba as usize * ss;
        if disk.data.len() < needed {
            disk.data.resize(needed, 0);
        }

        for (i, data) in partitions.iter().enumerate() {
            disk.write_at(part_specs[i].0 as usize * ss, data);
        }

        disk
    }

    /// Build a minimal GPT disk image with the given partition entries.
    fn build_gpt_disk(
        sector_size: u32,
        partitions: &[(u64, u64, Guid)], // (first_lba, last_lba, type_guid)
    ) -> MemDisk {
        let ss = sector_size as usize;
        // Need: LBA 0 (protective MBR), LBA 1 (GPT header), LBA 2+ (entries)
        let entry_sectors = (partitions.len() * size_of::<GptPartitionEntry>() + ss - 1) / ss;
        let total_sectors = 2 + entry_sectors + 64; // header + entries + some data space
        let mut disk = MemDisk::new(sector_size, total_sectors as u64);

        // Write GPT header at LBA 1
        let header = GptHeader {
            signature: *b"EFI PART",
            revision: U32::new(0x0001_0000),
            header_size: U32::new(size_of::<GptHeader>() as u32),
            crc32: U32::ZERO,
            reserved: U32::ZERO,
            current_lba: U64::new(1),
            backup_lba: U64::new(total_sectors as u64 - 1),
            first_usable_lba: U64::new(34),
            last_usable_lba: U64::new(total_sectors as u64 - 34),
            disk_guid: Guid::read_from_bytes(&[0xAA; 16]).unwrap().into(),
            partition_entry_lba: U64::new(2),
            num_partition_entries: U32::new(partitions.len() as u32),
            partition_entry_size: U32::new(size_of::<GptPartitionEntry>() as u32),
            partition_entry_crc32: U32::ZERO,
            _padding: U32::ZERO,
        };
        let header_bytes: &[u8] = zerocopy::IntoBytes::as_bytes(&header);
        disk.write_at(ss, header_bytes);

        // Write partition entries at LBA 2
        for (i, &(first_lba, last_lba, type_guid)) in partitions.iter().enumerate() {
            let entry = GptPartitionEntry {
                type_guid: type_guid.into(),
                unique_guid: Guid::read_from_bytes(&[i as u8 + 1; 16]).unwrap().into(),
                first_lba: U64::new(first_lba),
                last_lba: U64::new(last_lba),
                attributes: U64::ZERO,
                name: [U16::ZERO; 36],
            };
            let entry_bytes: &[u8] = zerocopy::IntoBytes::as_bytes(&entry);
            let offset = 2 * ss + i * size_of::<GptPartitionEntry>();
            disk.write_at(offset, entry_bytes);
        }

        disk
    }

    #[test]
    fn test_parse_gpt_two_partitions() {
        let linux_guid = Guid::read_from_bytes(&[
            0xAF, 0x3D, 0xC6, 0x0F, 0x83, 0x84, 0x72, 0x47, 0x8E, 0x79, 0x3D, 0x69, 0xD8, 0x47,
            0x7D, 0xE4,
        ])
        .unwrap();
        let mut disk = build_gpt_disk(
            512,
            &[
                (2048, 4095, linux_guid),  // partition 1: 2048 sectors
                (4096, 10239, linux_guid), // partition 2: 6144 sectors
            ],
        );

        let parts = parse_gpt(&mut disk, 512).unwrap();
        assert_eq!(parts.len(), 2);

        assert_eq!(parts[0].start_lba, 2048);
        assert_eq!(parts[0].size_lba, 2048);
        assert_eq!(parts[0].type_guid, linux_guid);

        assert_eq!(parts[1].start_lba, 4096);
        assert_eq!(parts[1].size_lba, 6144);
        assert_eq!(parts[1].type_guid, linux_guid);
    }

    #[test]
    fn test_parse_gpt_skips_empty_entries() {
        let linux_guid = Guid::read_from_bytes(&[0x01; 16]).unwrap();
        let mut disk = build_gpt_disk(
            512,
            &[
                (2048, 4095, linux_guid),
                (4096, 8191, Guid::default()), // empty type GUID — should be skipped
                (8192, 16383, linux_guid),
            ],
        );

        let parts = parse_gpt(&mut disk, 512).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].start_lba, 2048);
        assert_eq!(parts[1].start_lba, 8192);
    }

    #[test]
    fn test_parse_gpt_4k_sectors() {
        let guid = Guid::read_from_bytes(&[0x42; 16]).unwrap();
        let mut disk = build_gpt_disk(4096, &[(34, 1023, guid)]);

        let parts = parse_gpt(&mut disk, 4096).unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].start_lba, 34);
        assert_eq!(parts[0].size_lba, 990);
    }

    #[test]
    fn test_parse_gpt_bad_signature() {
        let mut disk = MemDisk::new(512, 128);
        // Write garbage at LBA 1 — no "EFI PART" signature
        disk.write_at(512, b"NOT GPT!");

        let result = parse_gpt(&mut disk, 512);
        assert!(matches!(result, Err(FsError::NotFound)));
    }

    #[test]
    fn test_parse_gpt_empty_disk() {
        let mut disk = MemDisk::new(512, 128);
        // All zeros — no signature
        let result = parse_gpt(&mut disk, 512);
        assert!(matches!(result, Err(FsError::NotFound)));
    }
}

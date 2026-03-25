// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.

//! Test utilities for the filesystem layer.

use super::base::{BlockDevice, FsError};

/// In-memory block device for testing.
pub struct MemDisk {
    pub data: Vec<u8>,
    sector_size: u32,
}

impl MemDisk {
    /// Create a zeroed disk with the given sector size and count.
    pub fn new(sector_size: u32, sector_count: u64) -> Self {
        Self {
            data: vec![0u8; (sector_count * sector_size as u64) as usize],
            sector_size,
        }
    }

    /// Write raw bytes at the given byte offset.
    pub fn write_at(&mut self, offset: usize, data: &[u8]) {
        self.data[offset..offset + data.len()].copy_from_slice(data);
    }
}

impl BlockDevice for MemDisk {
    fn read_sectors(&mut self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), FsError> {
        let ss = self.sector_size as usize;
        let count = count as usize;
        let start = lba
            .checked_mul(self.sector_size as u64)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or(FsError::Invalid)?;
        let len = count.checked_mul(ss).ok_or(FsError::Invalid)?;
        let end = start.checked_add(len).ok_or(FsError::Invalid)?;
        if end > self.data.len() || len > buf.len() {
            return Err(FsError::Invalid);
        }
        buf[..len].copy_from_slice(&self.data[start..end]);
        Ok(())
    }

    fn sector_size(&self) -> u32 {
        self.sector_size
    }

    fn sector_count(&self) -> u64 {
        self.data.len() as u64 / self.sector_size as u64
    }
}

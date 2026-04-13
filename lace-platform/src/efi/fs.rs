// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Julian Andres Klode <julian.klode@canonical.com>
//! UEFI-specific filesystem access.
//!
//! This module provides UEFI filesystem implementations for the common
//! filesystem interface defined in the parent fs module.
//! Currently supports:
//! - UEFI SimpleFileSystem (FAT/FAT32, typically ESP partitions)
//! - ext4 filesystems via DiskIO protocol

use crate::fs::base::{BlockDevice, DirEntry, File, Filesystem, FsError};
use crate::fs::probe::DiscoveredStorage;
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use uefi::Identify;
use uefi::proto::device_path::DevicePath;
use uefi::proto::media::file::File as UefiFileProto;

/// UEFI simple filesystem implementation.
pub struct UefiFilesystem {
    proto: uefi::boot::ScopedProtocol<uefi::proto::media::fs::SimpleFileSystem>,
}

impl UefiFilesystem {
    /// Create a new UEFI filesystem from a handle.
    pub fn new(handle: uefi::Handle) -> Result<Self, uefi::Error> {
        log::debug!(
            "[FS] Attempting to open UEFI SimpleFileSystem on handle {:?}",
            handle
        );
        let proto = uefi::boot::open_protocol_exclusive::<uefi::proto::media::fs::SimpleFileSystem>(
            handle,
        )?;
        log::debug!("[FS] Successfully opened UEFI SimpleFileSystem");
        Ok(Self { proto })
    }
}

impl Filesystem for UefiFilesystem {
    fn open_file(&mut self, path: &str) -> Result<Box<dyn File>, FsError> {
        let mut root = self.proto.open_volume()?;

        // Convert path to UEFI format (backslashes)
        let uefi_path = path.replace('/', "\\");
        let mut path_utf16: Vec<u16> = uefi_path.encode_utf16().collect();
        path_utf16.push(0);

        let cstr16 = uefi::CStr16::from_u16_with_nul(&path_utf16).map_err(|_| FsError::Invalid)?;

        let file_handle = root
            .open(
                cstr16,
                uefi::proto::media::file::FileMode::Read,
                uefi::proto::media::file::FileAttribute::empty(),
            )
            .map_err(|e| {
                if e.status() == uefi::Status::NOT_FOUND {
                    FsError::NotFound
                } else {
                    FsError::Io(e)
                }
            })?;

        let regular_file = file_handle
            .into_regular_file()
            .ok_or(FsError::NotDirectory)?;

        Ok(Box::new(UefiFile { file: regular_file }))
    }

    fn file_exists(&mut self, path: &str) -> bool {
        self.open_file(path).is_ok()
    }

    fn read_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, FsError> {
        let mut root = self.proto.open_volume()?;

        let uefi_path = path.replace('/', "\\");
        let mut path_utf16: Vec<u16> = uefi_path.encode_utf16().collect();
        path_utf16.push(0);

        let cstr16 = uefi::CStr16::from_u16_with_nul(&path_utf16).map_err(|_| FsError::Invalid)?;

        let dir_handle = root.open(
            cstr16,
            uefi::proto::media::file::FileMode::Read,
            uefi::proto::media::file::FileAttribute::empty(),
        )?;

        let mut dir = dir_handle.into_directory().ok_or(FsError::NotDirectory)?;

        let mut entries = Vec::new();
        let mut buf = vec![0u8; 512];

        loop {
            let info = match dir.read_entry(&mut buf) {
                Ok(Some(info)) => info,
                Ok(None) => break,
                Err(e) => return Err(FsError::Io(e.to_err_without_payload())),
            };

            let name = alloc::string::String::from_utf16_lossy(info.file_name().to_u16_slice());
            let is_dir = info
                .attribute()
                .contains(uefi::proto::media::file::FileAttribute::DIRECTORY);

            entries.push(DirEntry { name, is_dir });
        }

        Ok(entries)
    }
}

/// UEFI file handle.
struct UefiFile {
    file: uefi::proto::media::file::RegularFile,
}

impl File for UefiFile {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, FsError> {
        Ok(self.file.read(buf)?)
    }

    fn read_to_end(&mut self) -> Result<Vec<u8>, FsError> {
        let mut info_buf = vec![0u8; 256];
        let info = self
            .file
            .get_info::<uefi::proto::media::file::FileInfo>(&mut info_buf)
            .map_err(|e| FsError::Io(e.to_err_without_payload()))?;
        let size = info.file_size() as usize;
        let mut buf = vec![0u8; size];
        let bytes_read = self.file.read(&mut buf)?;
        if bytes_read != size {
            return Err(FsError::Io(uefi::Error::new(
                uefi::Status::DEVICE_ERROR,
                (),
            )));
        }
        Ok(buf)
    }

    fn size(&mut self) -> u64 {
        let mut info_buf = vec![0u8; 256];
        self.file
            .get_info::<uefi::proto::media::file::FileInfo>(&mut info_buf)
            .map(|info| info.file_size())
            .unwrap_or(0)
    }
}

/// UEFI DiskIO wrapper providing byte-oriented disk access.
pub struct UefiBlockDevice {
    disk_io: uefi::boot::ScopedProtocol<uefi::proto::media::disk::DiskIo>,
    block_io: uefi::boot::ScopedProtocol<uefi::proto::media::block::BlockIO>,
}

impl UefiBlockDevice {
    pub fn new(handle: uefi::Handle) -> Result<Self, uefi::Error> {
        let disk_io =
            uefi::boot::open_protocol_exclusive::<uefi::proto::media::disk::DiskIo>(handle)?;
        // BlockIO is opened non-exclusively for metadata only — opening it
        // exclusively would conflict with DiskIo which is an adapter on top of it.
        let block_io = unsafe {
            uefi::boot::open_protocol::<uefi::proto::media::block::BlockIO>(
                uefi::boot::OpenProtocolParams {
                    handle,
                    agent: uefi::boot::image_handle(),
                    controller: None,
                },
                uefi::boot::OpenProtocolAttributes::GetProtocol,
            )?
        };
        Ok(Self { disk_io, block_io })
    }
}

impl BlockDevice for UefiBlockDevice {
    fn read_sectors(&mut self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), FsError> {
        let media = self.block_io.media();
        let offset = lba * media.block_size() as u64;
        self.disk_io
            .read_disk(
                media.media_id(),
                offset,
                &mut buf[..count as usize * media.block_size() as usize],
            )
            .map_err(FsError::Io)
    }

    fn sector_size(&self) -> u32 {
        self.block_io.media().block_size()
    }

    fn sector_count(&self) -> u64 {
        self.block_io.media().last_block() + 1
    }

    fn read_bytes(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), FsError> {
        let media = self.block_io.media();
        self.disk_io
            .read_disk(media.media_id(), offset, buf)
            .map_err(FsError::Io)?;
        Ok(())
    }

    fn block_size(&self) -> u32 {
        self.block_io.media().block_size()
    }

    fn size(&self) -> u64 {
        let media = self.block_io.media();
        (media.last_block() + 1) * (media.block_size() as u64)
    }
}

/// Check if a handle is a logical partition via BlockIO media metadata.
fn is_logical_partition(handle: uefi::Handle) -> bool {
    // SAFETY: We're just checking media metadata and immediately closing the protocol.
    unsafe {
        match uefi::boot::open_protocol::<uefi::proto::media::block::BlockIO>(
            uefi::boot::OpenProtocolParams {
                handle,
                agent: uefi::boot::image_handle(),
                controller: None,
            },
            uefi::boot::OpenProtocolAttributes::GetProtocol,
        ) {
            Ok(proto) => {
                let is_part = proto.media().is_logical_partition();
                drop(proto);
                is_part
            }
            Err(_) => false,
        }
    }
}

/// Classify a single handle as a filesystem, partition, or neither.
fn classify_handle(handle: uefi::Handle, result: &mut DiscoveredStorage) {
    // Try UEFI SimpleFileSystem first (FAT/FAT32 on ESP)
    if let Ok(fs) = UefiFilesystem::new(handle) {
        log::debug!("[EFI] Found SimpleFileSystem on handle {:?}", handle);
        result.filesystems.push(Box::new(fs));
        return;
    }

    // For logical partitions, register as block devices
    if is_logical_partition(handle) {
        match UefiBlockDevice::new(handle) {
            Ok(dev) => {
                log::debug!("[EFI] Found partition block device: {:?}", handle);
                result.partitions.push(Box::new(dev));
            }
            Err(e) => {
                log::debug!("[EFI] Failed to open DiskIO on {:?}: {:?}", handle, e);
            }
        }
    }
}

/// Locate all DiskIO handles.
fn enumerate_disk_io_handles() -> Vec<uefi::Handle> {
    match uefi::boot::locate_handle_buffer(uefi::boot::SearchType::ByProtocol(
        &uefi::proto::media::disk::DiskIo::GUID,
    )) {
        Ok(h) => h.to_vec(),
        Err(e) => {
            log::debug!("[EFI] Failed to enumerate DiskIO handles: {:?}", e);
            Vec::new()
        }
    }
}

/// Get the device path bytes of a handle with the last node (partition) stripped.
///
/// This gives the "parent disk" device path, which can be compared against
/// other handles to find sibling partitions on the same disk.
fn get_parent_device_path_bytes(handle: uefi::Handle) -> Option<Vec<u8>> {
    // SAFETY: Just reading the device path, immediately closing.
    let dp = unsafe {
        uefi::boot::open_protocol::<DevicePath>(
            uefi::boot::OpenProtocolParams {
                handle,
                agent: uefi::boot::image_handle(),
                controller: None,
            },
            uefi::boot::OpenProtocolAttributes::GetProtocol,
        )
        .ok()?
    };

    // Collect all nodes except the last one
    let nodes: Vec<_> = dp.node_iter().collect();
    if nodes.len() < 2 {
        return None;
    }

    let mut bytes = Vec::new();
    for node in &nodes[..nodes.len() - 1] {
        let node_bytes = unsafe {
            core::slice::from_raw_parts(*node as *const _ as *const u8, node.length() as usize)
        };
        bytes.extend_from_slice(node_bytes);
    }
    Some(bytes)
}

/// Discover all storage.
pub fn discover_storage() -> DiscoveredStorage {
    let handles = enumerate_disk_io_handles();
    log::debug!("[EFI] Found {} DiskIO handles", handles.len());

    let mut result = DiscoveredStorage::new();

    for handle in handles {
        classify_handle(handle, &mut result);
    }

    result
}

/// Discover storage on the boot disk only.
///
/// Uses the `LoadedImage` protocol's `device_handle` to identify the boot
/// partition, then finds all sibling partitions on the same disk by comparing
/// device path prefixes.
pub fn discover_boot_storage() -> DiscoveredStorage {
    let mut result = DiscoveredStorage::new();

    // Get the boot partition handle from LoadedImage
    let boot_device_handle = {
        let li = match uefi::boot::open_protocol_exclusive::<uefi::proto::loaded_image::LoadedImage>(
            uefi::boot::image_handle(),
        ) {
            Ok(li) => li,
            Err(e) => {
                log::debug!("[EFI] Failed to open LoadedImage: {:?}", e);
                return result;
            }
        };
        match li.device() {
            Some(h) => h,
            None => {
                log::debug!("[EFI] LoadedImage has no device handle");
                return result;
            }
        }
    };

    log::debug!("[EFI] Boot device handle: {:?}", boot_device_handle);

    let boot_parent = get_parent_device_path_bytes(boot_device_handle);

    let handles = enumerate_disk_io_handles();
    log::debug!(
        "[EFI] Scanning {} handles for boot disk siblings",
        handles.len()
    );

    for handle in handles {
        let is_boot_device = handle == boot_device_handle;
        let is_sibling = boot_parent.as_ref().is_some_and(|boot_dp| {
            get_parent_device_path_bytes(handle)
                .as_ref()
                .is_some_and(|dp| dp == boot_dp)
        });

        if is_boot_device || is_sibling {
            classify_handle(handle, &mut result);
        }
    }

    result
}

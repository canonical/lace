// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! EFI image loading.

use super::mem::{AllocateType, MemoryType, PageAllocation, page_count};
use core::{ffi::c_void, fmt::Display};
use lace_util::peimage;

/// Represents a loaded EFI image.
pub struct LaceLoadedImage {
    pages: PageAllocation,
    image_size: usize,
    entry_point: usize,
}

/// Errors that can occur while loading an EFI image.
#[derive(Debug)]
pub enum LaceLoadImageError {
    PeError(peimage::PeError),
    MemoryAllocationError(uefi::Error),
}

impl Display for LaceLoadImageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LaceLoadImageError::PeError(e) => write!(f, "PE parsing error: {}", e),
            LaceLoadImageError::MemoryAllocationError(e) => {
                write!(f, "memory allocation error: {}", e)
            }
        }
    }
}

/// Raw representation of the UEFI Loaded Image Protocol.
/// This is needed so that we can locate it using uefi-rs but still modify its fields directly.
#[repr(transparent)]
struct RawLoadedImage(uefi_raw::protocol::loaded_image::LoadedImageProtocol);

unsafe impl uefi::Identify for RawLoadedImage {
    const GUID: uefi::Guid = uefi_raw::protocol::loaded_image::LoadedImageProtocol::GUID;
}

impl uefi::proto::Protocol for RawLoadedImage {}

impl LaceLoadedImage {
    /// Loads an EFI image from the given byte slice.
    pub fn load(image: &[u8]) -> Result<Self, LaceLoadImageError> {
        let pe = peimage::parse_pe(image).map_err(LaceLoadImageError::PeError)?;
        let mut pages = PageAllocation::new_uninit(
            AllocateType::AnyPages,
            MemoryType::LOADER_CODE,
            page_count!(pe.nt_hdrs.optional_header.size_of_image as usize),
        )
        .map_err(LaceLoadImageError::MemoryAllocationError)?;
        pe.relocate_into(pages.as_u8_slice_mut())
            .map_err(LaceLoadImageError::PeError)?;
        Ok(LaceLoadedImage {
            pages,
            image_size: pe.nt_hdrs.optional_header.size_of_image as usize,
            entry_point: pe.nt_hdrs.optional_header.address_of_entry_point as usize,
        })
    }

    /// Starts execution of the loaded EFI image.
    pub fn start(self, cmdline_utf16: Option<&[u16]>) -> ! {
        // Re-use parent loaded image and modify it to point to the new image base and size.
        let handle = uefi::boot::image_handle();
        let mut li = unsafe {
            uefi::boot::open_protocol::<RawLoadedImage>(
                uefi::boot::OpenProtocolParams {
                    handle,
                    agent: handle,
                    controller: None,
                },
                uefi::boot::OpenProtocolAttributes::GetProtocol,
            )
            // Let this panic here, this is not a condition that can happen on any
            // non completely broken UEFI implementation.
            .expect("cannot find our own loaded image")
        };

        // NOTE: from here on we modify the loaded image in-place, and shouldn't return.
        // If we wanted to be able to return, we would need to save and restore
        // the original values, but all fallible operations have already been done.
        li.0.device_handle = core::ptr::null_mut();
        li.0.file_path = core::ptr::null();
        li.0.image_base = self.pages.as_ptr() as *const c_void;
        li.0.image_size = self.image_size as u64;
        if let Some(cmdline_utf16) = cmdline_utf16 {
            // SAFETY: cmdline_utf16 lives through the rest of this function,
            // and at this point we can no longer return.
            li.0.load_options = cmdline_utf16.as_ptr() as *const c_void;
            li.0.load_options_size = core::mem::size_of_val(cmdline_utf16) as u32;
        }

        // Start the kernel image
        unsafe {
            // SAFETY: entry point is valid as we have relocated the image correctly.
            type EntryFn = extern "efiapi" fn(
                uefi_raw::Handle,
                *mut uefi_raw::table::system::SystemTable,
            ) -> uefi_raw::Status;
            let entry: EntryFn = core::mem::transmute(self.pages.as_ptr().add(self.entry_point));
            let _ = entry(
                handle.as_ptr(),
                uefi::table::system_table_raw().unwrap().as_mut(),
            );
        }
        panic!("fatal: kernel returned");
    }
}

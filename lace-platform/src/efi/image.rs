// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! EFI image loading.

use crate::mem::{
    self, MemAttributes, MemoryType, PageAllocation, PageAllocationConstraint, PageAllocationIface,
};
use core::ffi::c_void;
use lace_util::Display;
use lace_util::{align_up, peimage};

/// Represents a loaded EFI image.
pub struct LaceLoadedImage {
    pages: PageAllocation,
    image_size: usize,
    entry_point: usize,
}

/// Errors that can occur while loading an EFI image.
#[derive(Debug, Display)]
pub enum LaceLoadImageError {
    #[display("PE parsing error: {}")]
    PeError(peimage::PeError),
    #[display("NX compatibility policy violation")]
    NxPolicyViolation,
    #[display("memory allocation error: {}")]
    MemoryAllocationError(uefi::Error),
    #[display("failed to change memory attributes: {}")]
    MemoryAttributeError(uefi::Error),
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
        let nx_compat = pe.nt_hdrs.optional_header.dll_characteristics
            & peimage::DLLCHARACTERISTICS_NX_COMPAT
            != 0;

        if crate::mem::nx_required() && !nx_compat {
            // NX is required but the image is not NX compatible.
            return Err(LaceLoadImageError::NxPolicyViolation);
        }
        // NX compatibility requires page-aligned image size.
        if nx_compat
            && ((pe.nt_hdrs.optional_header.section_alignment as usize) < mem::PAGE_SIZE
                || !(pe.nt_hdrs.optional_header.section_alignment as usize)
                    .is_multiple_of(mem::PAGE_SIZE))
        {
            return Err(LaceLoadImageError::NxPolicyViolation);
        }

        let mut pages = PageAllocation::new_zeroed(
            PageAllocationConstraint::AnyAddress,
            Some(MemoryType::LOADER_CODE),
            mem::page_count(pe.nt_hdrs.optional_header.size_of_image as usize),
            None,
        )
        .map_err(LaceLoadImageError::MemoryAllocationError)?;

        let image_base = pages.as_ptr() as u64;
        let alloc_size = pages.pages() * mem::PAGE_SIZE;

        // If the image is NX compatible set the base policy as EXECUTE_PROTECT, otherwise no protection.
        let base_attrs = if nx_compat {
            MemAttributes::EXECUTE_PROTECT
        } else {
            MemAttributes::empty()
        };
        log::debug!("Setting base memory attributes: {:?}", base_attrs);
        mem::change_mem_attrs(image_base..(image_base + alloc_size as u64), base_attrs)
            .map_err(LaceLoadImageError::MemoryAttributeError)?;

        // Relocate the image into the allocated pages.
        pe.relocate_into(pages.as_u8_slice_mut())
            .map_err(LaceLoadImageError::PeError)?;

        if nx_compat {
            // Set PE header as read-only.
            let pe_header_size = align_up!(
                pe.nt_hdrs.optional_header.size_of_headers as u64,
                mem::PAGE_SIZE as u64
            );
            log::debug!("Setting PE header memory attributes: WRITE_PROTECT | EXECUTE_PROTECT");
            mem::change_mem_attrs(
                image_base..(image_base + pe_header_size),
                MemAttributes::WRITE_PROTECT | MemAttributes::EXECUTE_PROTECT,
            )
            .map_err(LaceLoadImageError::MemoryAttributeError)?;

            // Set section-wise memory attributes.
            // We call raw_sections() here because `pe` is still in raw format, because
            // it does not point to the relocated image. This is fine because we only
            // need the section headers.
            for section in pe.raw_sections() {
                let (shdr, _) = section.map_err(LaceLoadImageError::PeError)?;
                let sec_start = image_base + shdr.virtual_address as u64;
                let sec_end =
                    sec_start + align_up!(shdr.virtual_size as u64, mem::PAGE_SIZE as u64);

                // NX requires section start to be page-aligned.
                if sec_start != align_up!(sec_start, mem::PAGE_SIZE as u64) {
                    return Err(LaceLoadImageError::NxPolicyViolation);
                }
                // NX requires no W&X sections.
                if shdr.characteristics & peimage::SCN_MEM_WRITE != 0
                    && shdr.characteristics & peimage::SCN_MEM_EXECUTE != 0
                {
                    return Err(LaceLoadImageError::NxPolicyViolation);
                }

                // Set section attributes
                let mut attrs = MemAttributes::empty();
                if shdr.characteristics & peimage::SCN_MEM_READ == 0 {
                    // Not readable
                    attrs |= MemAttributes::READ_PROTECT;
                }
                if shdr.characteristics & peimage::SCN_MEM_WRITE == 0 {
                    // Not writable
                    attrs |= MemAttributes::WRITE_PROTECT;
                }
                if shdr.characteristics & peimage::SCN_MEM_EXECUTE == 0 {
                    // Not executable
                    attrs |= MemAttributes::EXECUTE_PROTECT;
                }

                log::debug!(
                    "Setting section memory attributes for section at {:#x}-{:#x}: {:?}",
                    sec_start,
                    sec_end,
                    attrs
                );
                mem::change_mem_attrs(sec_start..sec_end, attrs)
                    .map_err(LaceLoadImageError::MemoryAttributeError)?;
            }
        }

        log::debug!(
            "Loaded EFI image at {:p}, size {:#x}",
            pages.as_ptr(),
            pe.nt_hdrs.optional_header.size_of_image
        );

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

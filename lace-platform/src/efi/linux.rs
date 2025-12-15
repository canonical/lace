// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Linux bootloader support.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::{ffi::c_void, fmt::Display};

#[derive(Debug)]
pub enum BootLinuxError {
    LoadKernelError(uefi::Error),
    LoadInitrdError(uefi::Error),
    CmdlineUtf8Error,
    StartKernelError(uefi::Error),
}

impl Display for BootLinuxError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BootLinuxError::LoadKernelError(e) => write!(f, "failed to load kernel image: {}", e),
            BootLinuxError::LoadInitrdError(e) => write!(f, "failed to load initrd image: {}", e),
            BootLinuxError::CmdlineUtf8Error => write!(f, "command line is not valid UTF-8"),
            BootLinuxError::StartKernelError(e) => write!(f, "failed to start kernel image: {}", e),
        }
    }
}

pub fn boot_linux(
    kernel: &[u8],
    initrd: Option<&[u8]>,
    cmdline: Option<&[u8]>,
) -> Result<(), BootLinuxError> {
    // Load kernel image
    // TODO(mkukri): verification should be done via shim instead.
    let handle = uefi::boot::load_image(
        uefi::boot::image_handle(),
        uefi::boot::LoadImageSource::FromBuffer {
            buffer: kernel,
            file_path: None,
        },
    )
    .map_err(BootLinuxError::LoadKernelError)?;
    // Locate kernel loaded image protocol
    let mut li =
        uefi::boot::open_protocol_exclusive::<uefi::proto::loaded_image::LoadedImage>(handle)
            .map_err(BootLinuxError::LoadKernelError)?;

    // Load initrd (this will create a handle with a special device path the kernel searches for)
    let _initrd_loader = if let Some(initrd) = initrd {
        Some(InitrdLoader::load(initrd).map_err(BootLinuxError::LoadInitrdError)?)
    } else {
        None
    };

    // Install command line to kernel loaded image
    let _cmdline_utf16 = if let Some(cmdline) = cmdline {
        let mut cmdline_utf16: Vec<u16> = Vec::new();
        cmdline_utf16.extend(
            core::str::from_utf8(cmdline)
                .map_err(|_| BootLinuxError::CmdlineUtf8Error)?
                .encode_utf16(),
        );
        cmdline_utf16.push(0);

        unsafe {
            // SAFETY: _cmdline_utf16 lives through the rest of this function,
            // and the loaded image referencing it cannot escape this function.
            li.set_load_options(
                cmdline_utf16.as_ptr() as *const u8,
                (cmdline_utf16.len() * size_of::<u16>()) as u32,
            );
        }
        Some(cmdline_utf16)
    } else {
        None
    };

    // Start kernel image
    uefi::boot::start_image(handle).map_err(BootLinuxError::StartKernelError)?;
    panic!("fatal: kernel returned");
}

struct InitrdLoader<'initrd> {
    handle: uefi::Handle,
    lf2: Box<InitrdLf2<'initrd>>,
    dp: Box<InitrdMediaDp>,
}

impl<'initrd> InitrdLoader<'initrd> {
    fn load(initrd: &'initrd [u8]) -> Result<Self, uefi::Error> {
        let lf2 = InitrdLf2::new(initrd);
        let dp = InitrdMediaDp::new();
        let handle = unsafe {
            let handle = uefi::boot::install_protocol_interface(
                None,
                &uefi_raw::protocol::media::LoadFile2Protocol::GUID,
                &*lf2 as *const InitrdLf2 as *const c_void,
            )?;
            uefi::boot::install_protocol_interface(
                Some(handle),
                &uefi_raw::protocol::device_path::DevicePathProtocol::GUID,
                &*dp as *const InitrdMediaDp as *const c_void,
            )?;
            handle
        };
        Ok(Self { handle, lf2, dp })
    }
}

impl<'initrd> Drop for InitrdLoader<'initrd> {
    fn drop(&mut self) {
        unsafe {
            let _ = uefi::boot::uninstall_protocol_interface(
                self.handle,
                &uefi_raw::protocol::media::LoadFile2Protocol::GUID,
                &*self.lf2 as *const InitrdLf2 as *const c_void,
            );
            let _ = uefi::boot::uninstall_protocol_interface(
                self.handle,
                &uefi_raw::protocol::device_path::DevicePathProtocol::GUID,
                &*self.dp as *const InitrdMediaDp as *const c_void,
            );
        }
    }
}

#[repr(C)]
struct InitrdLf2<'initrd> {
    lf2: uefi_raw::protocol::media::LoadFile2Protocol,
    initrd: &'initrd [u8],
}

impl<'initrd> InitrdLf2<'initrd> {
    fn new(initrd: &'initrd [u8]) -> Box<Self> {
        Self {
            lf2: uefi_raw::protocol::media::LoadFile2Protocol {
                load_file: Self::efi_load_file,
            },
            initrd,
        }
        .into()
    }

    extern "efiapi" fn efi_load_file(
        this: *mut uefi_raw::protocol::media::LoadFile2Protocol,
        _file_path: *const uefi_raw::protocol::device_path::DevicePathProtocol,
        _boot_policy: uefi_raw::Boolean,
        buffer_size: *mut usize,
        buffer: *mut c_void,
    ) -> uefi_raw::Status {
        unsafe {
            let this = &mut *(this as *mut InitrdLf2);
            let initrd_len = this.initrd.len();
            if *buffer_size < initrd_len {
                *buffer_size = initrd_len;
                return uefi_raw::Status::BUFFER_TOO_SMALL;
            }
            core::slice::from_raw_parts_mut(buffer as *mut u8, initrd_len)
                .copy_from_slice(this.initrd);
            uefi_raw::Status::SUCCESS
        }
    }
}

#[repr(C, packed)]
struct InitrdMediaDp {
    /// Vendor media node
    ven: InitrdMediaVendorDp,
    /// End node
    end: uefi_raw::protocol::device_path::DevicePathProtocol,
}

#[repr(C, packed)]
struct InitrdMediaVendorDp {
    /// Node header
    hdr: uefi_raw::protocol::device_path::DevicePathProtocol,
    /// LINUX_EFI_INITRD_MEDIA_GUID
    guid: uefi_raw::Guid,
}

const LINUX_EFI_INITRD_MEDIA_GUID: uefi_raw::Guid =
    uefi_raw::guid!("5568e427-68fc-4f3d-ac74-ca555231cc68");

impl InitrdMediaDp {
    fn new() -> Box<Self> {
        Self {
            ven: InitrdMediaVendorDp {
                hdr: uefi_raw::protocol::device_path::DevicePathProtocol {
                    major_type: uefi::proto::device_path::DeviceType::MEDIA,
                    sub_type: uefi::proto::device_path::DeviceSubType::MEDIA_VENDOR,
                    length: [size_of::<InitrdMediaVendorDp>() as u8, 0],
                },
                guid: LINUX_EFI_INITRD_MEDIA_GUID,
            },
            end: uefi_raw::protocol::device_path::DevicePathProtocol {
                major_type: uefi::proto::device_path::DeviceType::END,
                sub_type: uefi::proto::device_path::DeviceSubType::END_ENTIRE,
                length: [
                    size_of::<uefi_raw::protocol::device_path::DevicePathProtocol>() as u8,
                    0,
                ],
            },
        }
        .into()
    }
}

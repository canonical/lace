// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
// UEFI main application

#![no_main]
#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::fmt;
use lace_util::peimage;
use lace_util::smbios::*;
use uefi::Guid;
use uefi::boot;
use uefi::boot::LoadImageSource;
use uefi::prelude::*;
use uefi::proto::loaded_image::LoadedImage;
use uefi::system;
use uefi::table::cfg::ConfigTableEntry;
use uefi_raw::protocol::device_path::DevicePathProtocol;
use uefi_raw::protocol::device_path::DeviceSubType;
use uefi_raw::protocol::device_path::DeviceType;
use uefi_raw::protocol::media::LoadFile2Protocol;

type Error = Box<ErrorStruct>;

#[derive(Debug, Clone)]
struct ErrorStruct(String);

impl fmt::Display for ErrorStruct {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

macro_rules! errorf {
    ($($args:tt)*) => {
        Box::new(ErrorStruct(format!($($args)*)))
    };
}

#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();
    uefi::println!("UEFI main()");

    // Parse SMBIOS
    let s: &'static _ = find_smbios_tables().unwrap();
    uefi::println!("SMBIOS table {:#?} {}", s.as_ptr(), s.len());
    system::with_stdout(|w| lace_util::hexdump(w, s)).unwrap();
    let smbios0 = find_smbios_table_by_type::<SmbiosTableType0>(s, 0)
        .ok()
        .flatten()
        .expect("need table 0");
    uefi::println!("{:#x?}", smbios0.table());
    let smbios1 = find_smbios_table_by_type::<SmbiosTableType1>(s, 1)
        .ok()
        .flatten()
        .expect("need table 1");
    uefi::println!("{:#x?}", smbios1.table());

    let not_found = find_smbios_table_by_type::<SmbiosHeader>(s, 42);
    assert!(matches!(not_found, Ok(None)));

    // Parse our own loaded image
    let li = boot::open_protocol_exclusive::<LoadedImage>(boot::image_handle())
        .expect("cannot get our own loaded image");
    let (ptr, len) = li.info();
    let li_slice = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let pe = peimage::parse_pe(li_slice).expect("failed to parse our own PE image");

    uefi::println!("{:#x?}", pe.nt_hdrs);
    uefi::println!("{:#x?}", pe.sect_hdrs);
    let mut kernel = None;
    let mut initrd = None;
    let mut cmdline = None;

    uefi::println!("PE sections");
    for result in pe.virtual_sections() {
        let (sect, data) = result.expect("failed to read section");
        uefi::println!(
            "  {:<8} {:08x} {:08x}",
            str::from_utf8(sect.name()).unwrap(),
            sect.virtual_address,
            sect.virtual_size
        );

        if sect.name() == b".linux" {
            kernel = Some(data);
        } else if sect.name() == b".initrd" {
            initrd = Some(data);
        } else if sect.name() == b".cmdline" {
            cmdline = Some(data);
        }
    }

    let kernel = kernel.expect("cannot boot without .linux section");

    boot_linux(kernel, initrd, cmdline).expect("failed to start linux");

    #[allow(clippy::empty_loop)]
    loop {}
}

fn find_smbios_tables() -> Option<&'static [u8]> {
    unsafe {
        if let Some(ptr) = find_config_table::<Smbios3EntryPoint>(ConfigTableEntry::SMBIOS3_GUID)
            && (*ptr).anchor_string.eq(b"_SM3_")
        {
            return Some(core::slice::from_raw_parts(
                (*ptr).table_address as _,
                (*ptr).table_maximum_size as _,
            ));
        }
        if let Some(ptr) = find_config_table::<SmbiosEntryPoint>(ConfigTableEntry::SMBIOS_GUID)
            && (*ptr).anchor_string.eq(b"_SM_")
        {
            return Some(core::slice::from_raw_parts(
                (*ptr).table_address as _,
                (*ptr).table_length as _,
            ));
        }
    }
    None
}

fn find_config_table<T>(guid: Guid) -> Option<*const T> {
    uefi::system::with_config_table(|tables| {
        for table in tables.iter() {
            if table.guid.eq(&guid) && !table.address.is_null() {
                return Some(table.address as *const T);
            }
        }
        None
    })
}

fn boot_linux(kernel: &[u8], initrd: Option<&[u8]>, cmdline: Option<&[u8]>) -> Result<(), Error> {
    // Load kernel image
    // TODO(mkukri): verification should be done via shim instead.
    let handle = boot::load_image(
        boot::image_handle(),
        LoadImageSource::FromBuffer {
            buffer: kernel,
            file_path: None,
        },
    )
    .map_err(|e| errorf!("failed to load kernel: {}", e))?;

    // Load initrd (this will create a handle with a special device path the kernel searches for)
    let _initrd_loader = InitrdLoader::load(initrd.unwrap_or(&[]))
        .map_err(|e| errorf!("failed to load initrd: {}", e))?;

    // Install command line to kernel loaded image
    let _cmdline_utf16 = if let Some(cmdline) = cmdline {
        let mut cmdline_utf16: Vec<u16> = Vec::new();
        cmdline_utf16.extend(
            str::from_utf8(cmdline)
                .map_err(|_| errorf!("cmdline is not valid utf8"))?
                .encode_utf16(),
        );
        cmdline_utf16.push(0);

        let mut li = boot::open_protocol_exclusive::<LoadedImage>(handle)
            .map_err(|e| errorf!("failed to locate kernel loaded image: {}", e))?;
        unsafe {
            // SAFETY: _cmdline_utf16 lives through the rest of this function,
            // and the loaded image referencing it cannot escape this function either.
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
    boot::start_image(handle).map_err(|e| errorf!("failed to start kernel image: {}", e))?;

    // Try to unload the kernel image
    let _ = boot::unload_image(handle);

    // If we reach here, something went wrong
    Err(errorf!("kernel image returned"))
}

struct InitrdLoader<'initrd> {
    handle: Handle,
    lf2: Box<InitrdLf2<'initrd>>,
    dp: Box<InitrdMediaDp>,
}

impl<'initrd> InitrdLoader<'initrd> {
    fn load(initrd: &'initrd [u8]) -> Result<Self, uefi::Error> {
        let lf2 = InitrdLf2::new(initrd);
        let dp = InitrdMediaDp::new();
        let handle = unsafe {
            let handle = boot::install_protocol_interface(
                None,
                &LoadFile2Protocol::GUID,
                &*lf2 as *const InitrdLf2 as *const c_void,
            )?;
            boot::install_protocol_interface(
                Some(handle),
                &DevicePathProtocol::GUID,
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
            let _ = boot::uninstall_protocol_interface(
                self.handle,
                &LoadFile2Protocol::GUID,
                &*self.lf2 as *const InitrdLf2 as *const c_void,
            );
            let _ = boot::uninstall_protocol_interface(
                self.handle,
                &DevicePathProtocol::GUID,
                &*self.dp as *const InitrdMediaDp as *const c_void,
            );
        }
    }
}

#[repr(C)]
struct InitrdLf2<'initrd> {
    lf2: LoadFile2Protocol,
    initrd: &'initrd [u8],
}

impl<'initrd> InitrdLf2<'initrd> {
    fn new(initrd: &'initrd [u8]) -> Box<Self> {
        Self {
            lf2: LoadFile2Protocol {
                load_file: Self::efi_load_file,
            },
            initrd,
        }
        .into()
    }

    extern "efiapi" fn efi_load_file(
        this: *mut LoadFile2Protocol,
        _file_path: *const DevicePathProtocol,
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
    end: DevicePathProtocol,
}

#[repr(C, packed)]
struct InitrdMediaVendorDp {
    /// Node header
    hdr: DevicePathProtocol,
    /// LINUX_EFI_INITRD_MEDIA_GUID
    guid: uefi_raw::Guid,
}

const LINUX_EFI_INITRD_MEDIA_GUID: uefi_raw::Guid =
    uefi_raw::guid!("5568e427-68fc-4f3d-ac74-ca555231cc68");

impl InitrdMediaDp {
    fn new() -> Box<Self> {
        Self {
            ven: InitrdMediaVendorDp {
                hdr: DevicePathProtocol {
                    major_type: DeviceType::MEDIA,
                    sub_type: DeviceSubType::MEDIA_VENDOR,
                    length: [size_of::<InitrdMediaVendorDp>() as u8, 0],
                },
                guid: LINUX_EFI_INITRD_MEDIA_GUID,
            },
            end: DevicePathProtocol {
                major_type: DeviceType::END,
                sub_type: DeviceSubType::END_ENTIRE,
                length: [size_of::<DevicePathProtocol>() as u8, 0],
            },
        }
        .into()
    }
}

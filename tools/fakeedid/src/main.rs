// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
// A simple UEFI application that installs a fake EDID protocol
// with data read from edid.bin file located at the root of the
// filesystem the application was booted from.

#![no_main]
#![no_std]

use core::ffi::c_void;
use uefi::Identify;
use uefi::proto::media::file::{File, RegularFile};
use uefi::{
    prelude::*,
    proto::{
        loaded_image::LoadedImage,
        media::{
            file::{FileAttribute, FileMode},
            fs::SimpleFileSystem,
        },
    },
};

/// This protocol contains the EDID information retrieved from a video output device.
/// Ref: 12.9.2.4. EFI_EDID_DISCOVERED_PROTOCOL
#[repr(C)]
struct EdidDiscoveredProtocol {
    size_of_edid: u32,
    edid: *const u8,
}

unsafe impl uefi::Identify for EdidDiscoveredProtocol {
    const GUID: uefi::Guid = uefi::guid!("1c0c34f6-d380-41fa-a049-8ad06c1a66aa");
}

impl uefi::proto::Protocol for EdidDiscoveredProtocol {}

#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();

    // Open edid.bin at the root of the filesystem fakeedid was booted from
    let li = boot::open_protocol_exclusive::<LoadedImage>(boot::image_handle())
        .expect("cannot get our own loaded image");

    let bootdev = li
        .device()
        .expect("cannot get boot device from loaded image");
    let mut bootfs = boot::open_protocol_exclusive::<SimpleFileSystem>(bootdev)
        .expect("cannot open SimpleFileSystem protocol on boot device");
    let mut root: uefi::proto::media::file::Directory =
        bootfs.open_volume().expect("cannot open root volume");
    let mut file = root
        .open(cstr16!("edid.bin"), FileMode::Read, FileAttribute::empty())
        .expect("cannot open edid.bin")
        .into_regular_file()
        .expect("edid.bin is not a regular file");

    // Allocate a buffer to store protocol and file and read the file into it
    file.set_position(RegularFile::END_OF_FILE)
        .expect("cannot seek to end of edid.bin");
    let file_size = file.get_position().expect("cannot get size of edid.bin");
    file.set_position(0)
        .expect("cannot seek to start of edid.bin");
    let mem = boot::allocate_pages(
        boot::AllocateType::AnyPages,
        boot::MemoryType::BOOT_SERVICES_DATA,
        (size_of::<EdidDiscoveredProtocol>() + file_size as usize).div_ceil(boot::PAGE_SIZE),
    )
    .expect("cannot allocate memory for edid.bin");

    unsafe {
        // Protocol first
        let ptr = mem.as_ptr() as *mut EdidDiscoveredProtocol;
        (*ptr).size_of_edid = file_size as u32;
        (*ptr).edid = mem.as_ptr().add(size_of::<EdidDiscoveredProtocol>());

        // Then file contents
        file.read(core::slice::from_raw_parts_mut(
            mem.as_ptr().add(size_of::<EdidDiscoveredProtocol>()),
            file_size as usize,
        ))
        .expect("cannot read edid.bin");

        // Install EDID protocol
        boot::install_protocol_interface(
            None,
            &EdidDiscoveredProtocol::GUID,
            mem.as_ptr() as *const c_void,
        )
        .expect("cannot install EDID protocol");
    };

    // Exit with error so firmware continues booting the next boot option
    Status::NOT_STARTED
}

// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

use core::{fmt::Display, mem::offset_of};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

pub const DOS_SIGNATURE: u16 = b'M' as u16 | (b'Z' as u16) << 8;
pub const NT_SIGNATURE: u32 = b'P' as u32 | (b'E' as u32) << 8;

#[repr(C)]
#[derive(Clone, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct DosHeader {
    pub e_magic: u16,
    pub e_cblp: u16,
    pub e_cp: u16,
    pub e_crlc: u16,
    pub e_cparhdr: u16,
    pub e_minalloc: u16,
    pub e_maxalloc: u16,
    pub e_ss: u16,
    pub e_sp: u16,
    pub e_csum: u16,
    pub e_ip: u16,
    pub e_cs: u16,
    pub e_lfarlc: u16,
    pub e_ovno: u16,
    pub e_res: [u16; 4],
    pub e_oemid: u16,
    pub e_oeminfo: u16,
    pub e_res2: [u16; 10],
    pub e_lfanew: u32,
}

#[repr(C)]
#[derive(Clone, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct FileHeader {
    pub machine: u16,
    pub number_of_sections: u16,
    pub time_date_stamp: u32,
    pub pointer_to_symbol_table: u32,
    pub number_of_symbols: u32,
    pub size_of_optional_header: u16,
    pub characteristics: u16,
}

#[repr(C)]
#[derive(Clone, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct DataDirectory {
    pub virtual_address: u32,
    pub size: u32,
}

pub const DIRECTORY_ENTRY_EXPORT: usize = 0;
pub const DIRECTORY_ENTRY_IMPORT: usize = 1;
pub const DIRECTORY_ENTRY_RESOURCE: usize = 2;
pub const DIRECTORY_ENTRY_EXCEPTION: usize = 3;
pub const DIRECTORY_ENTRY_SECURITY: usize = 4;
pub const DIRECTORY_ENTRY_BASERELOC: usize = 5;
pub const DIRECTORY_ENTRY_DEBUG: usize = 6;
pub const DIRECTORY_ENTRY_COPYRIGHT: usize = 7;
pub const DIRECTORY_ENTRY_GLOBALPTR: usize = 8;
pub const DIRECTORY_ENTRY_TLS: usize = 9;
pub const DIRECTORY_ENTRY_LOAD_CONFIG: usize = 10;

pub const NUMBER_OF_DIRECTORY_ENTRIES: usize = 16;

pub const NT_OPTIONAL_HDR64_MAGIC: u16 = 0x20b;

#[repr(C)]
#[derive(Clone, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct OptionalHeader64 {
    pub magic: u16,
    pub major_linker_version: u8,
    pub minor_linker_version: u8,
    pub size_of_code: u32,
    pub size_of_initialized_data: u32,
    pub size_of_uninitialized_data: u32,
    pub address_of_entry_point: u32,
    pub base_of_code: u32,
    pub image_base: u64,
    pub section_alignment: u32,
    pub file_alignment: u32,
    pub major_operating_system_version: u16,
    pub minor_operating_system_version: u16,
    pub major_image_version: u16,
    pub minor_image_version: u16,
    pub major_subsystem_version: u16,
    pub minor_subsystem_version: u16,
    pub win32_version_value: u32,
    pub size_of_image: u32,
    pub size_of_headers: u32,
    pub check_sum: u32,
    pub subsystem: u16,
    pub dll_characteristics: u16,
    pub size_of_stack_reserve: u64,
    pub size_of_stack_commit: u64,
    pub size_of_heap_reserve: u64,
    pub size_of_heap_commit: u64,
    pub loader_flags: u32,
    pub number_of_rva_and_sizes: u32,
    pub data_directory: [DataDirectory; NUMBER_OF_DIRECTORY_ENTRIES],
}

#[repr(C)]
#[derive(Clone, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct NtHeaders64 {
    pub signature: u32,
    pub file_header: FileHeader,
    pub optional_header: OptionalHeader64,
}

#[repr(C)]
#[derive(Clone, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct SectionHeader {
    pub name: [u8; 8],
    pub virtual_size: u32,
    pub virtual_address: u32,
    pub size_of_raw_data: u32,
    pub pointer_to_raw_data: u32,
    pub pointer_to_relocations: u32,
    pub pointer_to_linenumbers: u32,
    pub number_of_relocations: u16,
    pub number_of_linenumbers: u16,
    pub characteristics: u32,
}

pub const SCN_CNT_CODE: u32 = 0x00000020;
pub const SCN_CNT_INITIALIZED_DATA: u32 = 0x00000040;
pub const SCN_CNT_UNINITIALIZED_DATA: u32 = 0x00000080;

pub const SCN_MEM_DISCARDABLE: u32 = 0x02000000;
pub const SCN_MEM_NOT_CACHED: u32 = 0x04000000;
pub const SCN_MEM_NOT_PAGED: u32 = 0x08000000;
pub const SCN_MEM_SHARED: u32 = 0x10000000;
pub const SCN_MEM_EXECUTE: u32 = 0x20000000;
pub const SCN_MEM_READ: u32 = 0x40000000;
pub const SCN_MEM_WRITE: u32 = 0x80000000;

impl SectionHeader {
    pub fn name(&self) -> &[u8] {
        let mut end_i = 0;
        while end_i < self.name.len() && self.name[end_i] != 0 {
            end_i += 1;
        }
        &self.name[..end_i]
    }
}

#[derive(Clone, Debug)]
pub struct PeRef<'a> {
    pub data: &'a [u8],
    pub dos_hdr: &'a DosHeader,
    pub dos_data: &'a [u8],
    pub nt_hdrs: &'a NtHeaders64,
    pub nt_data: &'a [u8],
    pub sect_hdrs: &'a [SectionHeader],
}

impl<'a> PeRef<'a> {
    pub fn find_section(&self, name: &str) -> Option<&'a SectionHeader> {
        self.sect_hdrs
            .iter()
            .find(|&sect| sect.name().eq(name.as_bytes()))
    }
}

#[derive(Clone, Copy, Debug)]
pub enum PeParseError {
    Truncated,
    BadHeader,
}

impl Display for PeParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Truncated => write!(f, "image truncated"),
            Self::BadHeader => write!(f, "image has bad header"),
        }
    }
}

pub fn parse_pe<'a>(s: &'a [u8]) -> Result<PeRef<'a>, PeParseError> {
    // Get and validate the DOS header
    let Ok((dos_hdr, dos_data)) = DosHeader::ref_from_prefix(s) else {
        return Err(PeParseError::Truncated);
    };
    if dos_hdr.e_magic != DOS_SIGNATURE {
        return Err(PeParseError::BadHeader);
    }

    // Get and validate the 64-bit NT headers
    let Some((nt_hdrs, nt_data)) = s
        .get(dos_hdr.e_lfanew as usize..)
        .and_then(|s| NtHeaders64::ref_from_prefix(s).ok())
    else {
        return Err(PeParseError::Truncated);
    };
    if nt_hdrs.signature != NT_SIGNATURE {
        return Err(PeParseError::BadHeader);
    }
    if (nt_hdrs.file_header.size_of_optional_header as usize) < size_of::<OptionalHeader64>() {
        return Err(PeParseError::Truncated);
    }
    if nt_hdrs.optional_header.magic != NT_OPTIONAL_HDR64_MAGIC {
        return Err(PeParseError::BadHeader);
    }

    // Get the section headers as a slice
    let Some((sect_hdrs, _)) = (dos_hdr.e_lfanew as usize)
        .checked_add(offset_of!(NtHeaders64, optional_header))
        .and_then(|x| x.checked_add(nt_hdrs.file_header.size_of_optional_header as usize))
        .and_then(|x| s.get(x..))
        .and_then(|s| {
            <[SectionHeader]>::ref_from_prefix_with_elems(
                s,
                nt_hdrs.file_header.number_of_sections as usize,
            )
            .ok()
        })
    else {
        return Err(PeParseError::Truncated);
    };

    Ok(PeRef {
        data: s,
        dos_hdr,
        dos_data: &dos_data[..dos_hdr.e_lfanew as usize - size_of::<DosHeader>()],
        nt_hdrs,
        nt_data: &nt_data[..nt_hdrs.file_header.size_of_optional_header as usize
            - size_of::<OptionalHeader64>()],
        sect_hdrs,
    })
}

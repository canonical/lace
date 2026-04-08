// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

use crate::align_up;
use core::mem::offset_of;
use lace_util_derive::Display;
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
    // This struct in reality has a flexible length array here,
    // the length is given by 'number_of_rva_and_sizes'.
    // We are not using it for now, so we omit it.
    // pub data_directory: [DataDirectory; NUMBER_OF_DIRECTORY_ENTRIES],
}

/// DLL Characteristics flags
pub const DLLCHARACTERISTICS_NX_COMPAT: u16 = 0x0100;

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
    pub dos_hdr: DosHeader,
    pub dos_data: &'a [u8],
    pub nt_hdrs: NtHeaders64,
    pub nt_data: &'a [u8],
    pub sect_hdrs: &'a [u8],
}

pub struct RawSectionIterator<'a> {
    pe: PeRef<'a>,
    index: usize,
}

pub struct VirtualSectionIterator<'a> {
    pe: PeRef<'a>,
    index: usize,
}

#[derive(Clone, Copy, Debug, Display)]
pub enum PeError {
    #[display("image truncated")]
    Truncated,
    #[display("image has bad header")]
    BadHeader,
    #[display("image has relocations, which are not yet supported")]
    RelocationsNotYetSupported,
}

pub fn parse_pe<'a>(s: &'a [u8]) -> Result<PeRef<'a>, PeError> {
    // Get and validate the DOS header
    let (dos_hdr, dos_data) = DosHeader::read_from_prefix(s).map_err(|_| PeError::Truncated)?;
    if dos_hdr.e_magic != DOS_SIGNATURE {
        return Err(PeError::BadHeader);
    }

    // Get and validate the 64-bit NT headers
    let (nt_hdrs, nt_data) = s
        .get(dos_hdr.e_lfanew as usize..)
        .and_then(|s| NtHeaders64::read_from_prefix(s).ok())
        .ok_or(PeError::Truncated)?;
    if nt_hdrs.signature != NT_SIGNATURE {
        return Err(PeError::BadHeader);
    }
    if (nt_hdrs.file_header.size_of_optional_header as usize) < size_of::<OptionalHeader64>() {
        return Err(PeError::Truncated);
    }
    if nt_hdrs.optional_header.magic != NT_OPTIONAL_HDR64_MAGIC {
        return Err(PeError::BadHeader);
    }

    // Get the section headers as a slice
    let sect_hdrs_start = (dos_hdr.e_lfanew as usize)
        .checked_add(offset_of!(NtHeaders64, optional_header))
        .and_then(|x| x.checked_add(nt_hdrs.file_header.size_of_optional_header as usize))
        .ok_or(PeError::Truncated)?;
    let sect_hdrs_end = (nt_hdrs.file_header.number_of_sections as usize)
        .checked_mul(size_of::<SectionHeader>())
        .and_then(|size| sect_hdrs_start.checked_add(size))
        .ok_or(PeError::Truncated)?;
    let sect_hdrs = s
        .get(sect_hdrs_start..sect_hdrs_end)
        .ok_or(PeError::Truncated)?;

    let dos_data = &dos_data[..dos_hdr.e_lfanew as usize - size_of::<DosHeader>()];
    let nt_data = &nt_data
        [..nt_hdrs.file_header.size_of_optional_header as usize - size_of::<OptionalHeader64>()];
    Ok(PeRef {
        data: s,
        dos_hdr,
        dos_data,
        nt_hdrs,
        nt_data,
        sect_hdrs,
    })
}

impl<'a> PeRef<'a> {
    pub fn num_sections(&self) -> usize {
        self.nt_hdrs.file_header.number_of_sections as usize
    }

    pub fn nth_section(&self, n: usize) -> Option<SectionHeader> {
        if n >= self.num_sections() {
            return None;
        }
        // NOTE: 'unwrap()' cannot actually panic here because self.sect_hdrs has size
        // exactly of `self.num_sections() * size_of::<SectionHeader>()`, and 'n' is
        // guaranteed to be less than 'self.num_sections()', so there is always enough of
        // data left to read a SectionHeader.
        let (shdr, _) =
            SectionHeader::read_from_prefix(&self.sect_hdrs[n * size_of::<SectionHeader>()..])
                .unwrap();
        Some(shdr)
    }

    pub fn virtual_sections(&self) -> VirtualSectionIterator<'a> {
        VirtualSectionIterator {
            pe: self.clone(),
            index: 0,
        }
    }

    pub fn raw_sections(&self) -> RawSectionIterator<'a> {
        RawSectionIterator {
            pe: self.clone(),
            index: 0,
        }
    }

    /// Relocates the PE image into the provided memory slice.
    /// The slice must be at least as large as the image size specified
    /// in the optional header.
    pub fn relocate_into(&self, pages: &mut [u8]) -> Result<(), PeError> {
        let opt_hdr = &self.nt_hdrs.optional_header;

        // Copy headers to the allocated memory
        let hdrs_src = self
            .data
            .get(..opt_hdr.size_of_headers as usize)
            .ok_or(PeError::Truncated)?;
        pages
            .get_mut(..opt_hdr.size_of_headers as usize)
            .ok_or(PeError::Truncated)?
            .copy_from_slice(hdrs_src);

        // Copy sections to the allocated memory
        for result in self.raw_sections() {
            let (shdr, data) = result?;
            if shdr.pointer_to_relocations != 0 {
                return Err(PeError::RelocationsNotYetSupported);
            }

            // Virtual size must be aligned to section alignment, the linker is not required to align this for us.
            let virt_size = align_up!(
                shdr.virtual_size,
                self.nt_hdrs.optional_header.section_alignment
            ) as usize;
            if data.len() > virt_size {
                return Err(PeError::Truncated);
            }
            let virt_start = shdr.virtual_address as usize;
            let virt_end = virt_start
                .checked_add(virt_size)
                .ok_or(PeError::Truncated)?;

            // Copy initialized data
            pages
                .get_mut(virt_start..virt_start + data.len())
                .ok_or(PeError::Truncated)?
                .copy_from_slice(data);

            // Zero uninitialized data
            if data.len() < virt_size {
                pages
                    .get_mut((virt_start + data.len())..virt_end)
                    .ok_or(PeError::Truncated)?
                    .fill(0);
            }
        }

        Ok(())
    }
}

impl<'a> Iterator for RawSectionIterator<'a> {
    type Item = Result<(SectionHeader, &'a [u8]), PeError>;

    fn next(&mut self) -> Option<Self::Item> {
        let shdr = self.pe.nth_section(self.index)?;
        self.index += 1;

        (shdr.pointer_to_raw_data as usize)
            .checked_add(shdr.size_of_raw_data as usize)
            .and_then(|end_of_raw_data| {
                self.pe
                    .data
                    .get(shdr.pointer_to_raw_data as usize..end_of_raw_data)
                    .map(|data| Ok((shdr, data)))
                    .or(Some(Err(PeError::Truncated)))
            })
    }
}

impl<'a> Iterator for VirtualSectionIterator<'a> {
    type Item = Result<(SectionHeader, &'a [u8]), PeError>;

    fn next(&mut self) -> Option<Self::Item> {
        let shdr = self.pe.nth_section(self.index)?;
        self.index += 1;

        (shdr.virtual_address as usize)
            .checked_add(shdr.virtual_size as usize)
            .and_then(|end_of_virtual_section| {
                self.pe
                    .data
                    .get(shdr.virtual_address as usize..end_of_virtual_section)
                    .map(|data| Ok((shdr, data)))
                    .or(Some(Err(PeError::Truncated)))
            })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use core::mem::{offset_of, size_of};
    use zerocopy::IntoBytes;

    // Offsets for a PE with e_lfanew = 64.  All derived from struct sizes so
    // they stay correct if the structs ever change.
    const E_LFANEW: u32 = 64;
    const NT_SIG_OFF: usize = E_LFANEW as usize;
    const FILE_HDR_OFF: usize = NT_SIG_OFF + 4;
    const OPT_HDR_OFF: usize = FILE_HDR_OFF + size_of::<FileHeader>();
    const SECT_HDR_OFF: usize = OPT_HDR_OFF + size_of::<OptionalHeader64>();

    /// Write a `u16` little-endian at `buf[off..]`.
    fn write_u16(buf: &mut [u8], off: usize, val: u16) {
        buf[off..off + 2].copy_from_slice(&val.to_le_bytes());
    }

    /// Write a `u32` little-endian at `buf[off..]`.
    fn write_u32(buf: &mut [u8], off: usize, val: u32) {
        buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
    }

    /// Build a byte vector that contains a valid minimal PE64 image.
    ///
    /// `sections` is a list of `(name, virtual_address, raw_data)` tuples.
    /// Raw section data is appended after the header block (aligned to
    /// `file_alignment`).  The `virtual_address` fields are set as provided
    /// and are independent of the raw data placement.
    fn build_pe(sections: &[([u8; 8], u32, Vec<u8>)]) -> Vec<u8> {
        let file_alignment: u32 = 512;
        let section_alignment: u32 = 4096;
        let num_sections = sections.len() as u16;

        // Size of all headers (DOS + NT + section headers), rounded up to
        // file_alignment.
        let raw_hdr_size = SECT_HDR_OFF + num_sections as usize * size_of::<SectionHeader>();
        let size_of_headers = (raw_hdr_size as u32).div_ceil(file_alignment) * file_alignment;

        // Work out where each section's raw data lands in the file and build
        // the section headers.
        let mut section_headers: Vec<SectionHeader> = Vec::new();
        let mut next_raw_offset = size_of_headers;
        for (name, vaddr, raw_data) in sections {
            let size_of_raw_data = raw_data.len() as u32;
            let pointer_to_raw_data = if size_of_raw_data > 0 {
                next_raw_offset
            } else {
                0
            };
            let raw_rounded = size_of_raw_data.div_ceil(file_alignment) * file_alignment;
            next_raw_offset += raw_rounded;

            section_headers.push(SectionHeader {
                name: *name,
                virtual_size: size_of_raw_data,
                virtual_address: *vaddr,
                size_of_raw_data,
                pointer_to_raw_data,
                pointer_to_relocations: 0,
                pointer_to_linenumbers: 0,
                number_of_relocations: 0,
                number_of_linenumbers: 0,
                characteristics: SCN_CNT_INITIALIZED_DATA | SCN_MEM_READ,
            });
        }

        let total_file_size = next_raw_offset as usize;
        let mut buf = vec![0u8; total_file_size];

        // DOS header
        let dos_hdr = DosHeader {
            e_magic: DOS_SIGNATURE,
            e_cblp: 0,
            e_cp: 0,
            e_crlc: 0,
            e_cparhdr: 0,
            e_minalloc: 0,
            e_maxalloc: 0,
            e_ss: 0,
            e_sp: 0,
            e_csum: 0,
            e_ip: 0,
            e_cs: 0,
            e_lfarlc: 0,
            e_ovno: 0,
            e_res: [0; 4],
            e_oemid: 0,
            e_oeminfo: 0,
            e_res2: [0; 10],
            e_lfanew: E_LFANEW,
        };
        buf[0..size_of::<DosHeader>()].copy_from_slice(dos_hdr.as_bytes());

        // NT signature
        buf[NT_SIG_OFF..NT_SIG_OFF + 4].copy_from_slice(&NT_SIGNATURE.to_le_bytes());

        // FileHeader
        let file_hdr = FileHeader {
            machine: 0,
            number_of_sections: num_sections,
            time_date_stamp: 0,
            pointer_to_symbol_table: 0,
            number_of_symbols: 0,
            size_of_optional_header: size_of::<OptionalHeader64>() as u16,
            characteristics: 0,
        };
        buf[FILE_HDR_OFF..FILE_HDR_OFF + size_of::<FileHeader>()]
            .copy_from_slice(file_hdr.as_bytes());

        // OptionalHeader64
        let size_of_image = section_headers
            .iter()
            .map(|s| s.virtual_address + s.virtual_size)
            .max()
            .unwrap_or(0);
        let size_of_image = size_of_image.div_ceil(section_alignment) * section_alignment;
        let size_of_image = size_of_image.max(size_of_headers);

        let opt_hdr = OptionalHeader64 {
            magic: NT_OPTIONAL_HDR64_MAGIC,
            major_linker_version: 0,
            minor_linker_version: 0,
            size_of_code: 0,
            size_of_initialized_data: 0,
            size_of_uninitialized_data: 0,
            address_of_entry_point: 0,
            base_of_code: 0,
            image_base: 0,
            section_alignment,
            file_alignment,
            major_operating_system_version: 0,
            minor_operating_system_version: 0,
            major_image_version: 0,
            minor_image_version: 0,
            major_subsystem_version: 0,
            minor_subsystem_version: 0,
            win32_version_value: 0,
            size_of_image,
            size_of_headers,
            check_sum: 0,
            subsystem: 0,
            dll_characteristics: 0,
            size_of_stack_reserve: 0,
            size_of_stack_commit: 0,
            size_of_heap_reserve: 0,
            size_of_heap_commit: 0,
            loader_flags: 0,
            number_of_rva_and_sizes: 0,
        };
        buf[OPT_HDR_OFF..OPT_HDR_OFF + size_of::<OptionalHeader64>()]
            .copy_from_slice(opt_hdr.as_bytes());

        // Section headers
        for (i, shdr) in section_headers.iter().enumerate() {
            let off = SECT_HDR_OFF + i * size_of::<SectionHeader>();
            buf[off..off + size_of::<SectionHeader>()].copy_from_slice(shdr.as_bytes());
        }

        // Raw section data
        for (shdr, (_, _, raw_data)) in section_headers.iter().zip(sections.iter()) {
            if !raw_data.is_empty() {
                let off = shdr.pointer_to_raw_data as usize;
                buf[off..off + raw_data.len()].copy_from_slice(raw_data);
            }
        }

        buf
    }

    // ── parse_pe error paths ──────────────────────────────────────────────

    #[test]
    fn test_parse_pe_truncated_empty() {
        let result = parse_pe(&[]);
        assert!(matches!(result, Err(PeError::Truncated)));
    }

    #[test]
    fn test_parse_pe_bad_dos_magic() {
        // Valid-length buffer but wrong DOS magic
        let buf = vec![0u8; 256];
        let result = parse_pe(&buf);
        assert!(matches!(result, Err(PeError::BadHeader)));
    }

    #[test]
    fn test_parse_pe_bad_nt_signature() {
        // Start from a valid PE, corrupt the NT signature
        let mut buf = build_pe(&[]);
        write_u32(&mut buf, NT_SIG_OFF, 0xDEADBEEF);
        let result = parse_pe(&buf);
        assert!(matches!(result, Err(PeError::BadHeader)));
    }

    #[test]
    fn test_parse_pe_bad_optional_header_magic() {
        // Start from a valid PE, set optional header magic to wrong value
        let mut buf = build_pe(&[]);
        write_u16(&mut buf, OPT_HDR_OFF, 0x010b); // PE32, not PE32+
        let result = parse_pe(&buf);
        assert!(matches!(result, Err(PeError::BadHeader)));
    }

    #[test]
    fn test_parse_pe_optional_header_too_small() {
        // size_of_optional_header smaller than OptionalHeader64
        let mut buf = build_pe(&[]);
        write_u16(
            &mut buf,
            FILE_HDR_OFF + offset_of!(FileHeader, size_of_optional_header),
            (size_of::<OptionalHeader64>() - 1) as u16,
        );
        let result = parse_pe(&buf);
        assert!(matches!(result, Err(PeError::Truncated)));
    }

    #[test]
    fn test_parse_pe_e_lfanew_too_large() {
        // e_lfanew points past the end of the buffer
        let mut buf = build_pe(&[]);
        let past_end = buf.len() as u32 + 1;
        write_u32(&mut buf, offset_of!(DosHeader, e_lfanew), past_end);
        let result = parse_pe(&buf);
        assert!(matches!(result, Err(PeError::Truncated)));
    }

    // ── valid PE parsing ──────────────────────────────────────────────────

    #[test]
    fn test_parse_pe_valid_no_sections() {
        let buf = build_pe(&[]);
        let pe = parse_pe(&buf).expect("should parse valid PE");
        assert_eq!(pe.num_sections(), 0);
    }

    #[test]
    fn test_parse_pe_valid_with_sections() {
        let raw = b"Hello, PE!".to_vec();
        let sections = [(*b".text\0\0\0", 0x1000u32, raw)];
        let buf = build_pe(&sections);
        let pe = parse_pe(&buf).expect("should parse valid PE");
        assert_eq!(pe.num_sections(), 1);
    }

    #[test]
    fn test_parse_pe_preserves_dos_header_fields() {
        let buf = build_pe(&[]);
        let pe = parse_pe(&buf).expect("should parse");
        assert_eq!(pe.dos_hdr.e_magic, DOS_SIGNATURE);
        assert_eq!(pe.dos_hdr.e_lfanew, E_LFANEW);
    }

    #[test]
    fn test_parse_pe_preserves_nt_signature() {
        let buf = build_pe(&[]);
        let pe = parse_pe(&buf).expect("should parse");
        assert_eq!(pe.nt_hdrs.signature, NT_SIGNATURE);
    }

    // ── SectionHeader::name ───────────────────────────────────────────────

    #[test]
    fn test_section_header_name_null_terminated() {
        let shdr = SectionHeader {
            name: *b".text\0\0\0",
            virtual_size: 0,
            virtual_address: 0,
            size_of_raw_data: 0,
            pointer_to_raw_data: 0,
            pointer_to_relocations: 0,
            pointer_to_linenumbers: 0,
            number_of_relocations: 0,
            number_of_linenumbers: 0,
            characteristics: 0,
        };
        assert_eq!(shdr.name(), b".text");
    }

    #[test]
    fn test_section_header_name_full_eight_bytes() {
        // A name that uses all 8 bytes with no null terminator
        let shdr = SectionHeader {
            name: *b".rodata.",
            virtual_size: 0,
            virtual_address: 0,
            size_of_raw_data: 0,
            pointer_to_raw_data: 0,
            pointer_to_relocations: 0,
            pointer_to_linenumbers: 0,
            number_of_relocations: 0,
            number_of_linenumbers: 0,
            characteristics: 0,
        };
        assert_eq!(shdr.name(), b".rodata.");
    }

    #[test]
    fn test_section_header_name_all_zeros() {
        let shdr = SectionHeader {
            name: [0u8; 8],
            virtual_size: 0,
            virtual_address: 0,
            size_of_raw_data: 0,
            pointer_to_raw_data: 0,
            pointer_to_relocations: 0,
            pointer_to_linenumbers: 0,
            number_of_relocations: 0,
            number_of_linenumbers: 0,
            characteristics: 0,
        };
        assert_eq!(shdr.name(), b"");
    }

    // ── PeRef::nth_section / num_sections ─────────────────────────────────

    #[test]
    fn test_nth_section_out_of_bounds_returns_none() {
        let buf = build_pe(&[]);
        let pe = parse_pe(&buf).expect("should parse");
        assert!(pe.nth_section(0).is_none());
    }

    #[test]
    fn test_nth_section_returns_correct_header() {
        let sections = [
            (*b".text\0\0\0", 0x1000u32, vec![0xCCu8; 16]),
            (*b".data\0\0\0", 0x2000u32, vec![0xAAu8; 8]),
        ];
        let buf = build_pe(&sections);
        let pe = parse_pe(&buf).expect("should parse");

        assert_eq!(pe.num_sections(), 2);

        let s0 = pe.nth_section(0).expect("section 0 should exist");
        assert_eq!(s0.name(), b".text");
        assert_eq!(s0.virtual_address, 0x1000);

        let s1 = pe.nth_section(1).expect("section 1 should exist");
        assert_eq!(s1.name(), b".data");
        assert_eq!(s1.virtual_address, 0x2000);

        assert!(pe.nth_section(2).is_none());
    }

    // ── RawSectionIterator ────────────────────────────────────────────────

    #[test]
    fn test_raw_sections_no_sections() {
        let buf = build_pe(&[]);
        let pe = parse_pe(&buf).expect("should parse");
        let results: Vec<_> = pe.raw_sections().collect();
        assert!(results.is_empty());
    }

    #[test]
    fn test_raw_sections_correct_data() {
        let raw0 = b"KERNEL__".to_vec();
        let raw1 = b"INITRD__".to_vec();
        let sections = [
            (*b".kern\0\0\0", 0x1000u32, raw0.clone()),
            (*b".init\0\0\0", 0x2000u32, raw1.clone()),
        ];
        let buf = build_pe(&sections);
        let pe = parse_pe(&buf).expect("should parse");

        let results: Vec<_> = pe.raw_sections().map(|r| r.expect("no error")).collect();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].1, raw0.as_slice());
        assert_eq!(results[1].1, raw1.as_slice());
    }

    // ── VirtualSectionIterator ────────────────────────────────────────────

    #[test]
    fn test_virtual_sections_no_sections() {
        let buf = build_pe(&[]);
        let pe = parse_pe(&buf).expect("should parse");
        let results: Vec<_> = pe.virtual_sections().collect();
        assert!(results.is_empty());
    }

    #[test]
    fn test_virtual_sections_correct_data() {
        // virtual_address = 0 points into the PE file at offset 0, where the
        // DOS magic lives.  virtual_sections() must return Ok with the correct
        // bytes.
        let sections = [(*b".text\0\0\0", 0u32, vec![0u8; 8])];
        let buf = build_pe(&sections);
        let pe = parse_pe(&buf).expect("should parse");

        let results: Vec<_> = pe
            .virtual_sections()
            .map(|r| r.expect("no error"))
            .collect();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.virtual_address, 0);
        // virtual_size == size_of_raw_data == 8, so eight bytes are returned
        assert_eq!(results[0].1.len(), 8);
        // The first two bytes at file offset 0 are the DOS magic (MZ)
        assert_eq!(
            u16::from_le_bytes([results[0].1[0], results[0].1[1]]),
            DOS_SIGNATURE
        );
    }

    #[test]
    fn test_virtual_sections_out_of_bounds_returns_truncated() {
        // virtual_address = 0 is within the buffer; confirm the iterator
        // yields Ok before corruption.
        let sections = [(*b".text\0\0\0", 0u32, vec![0u8; 8])];
        let mut buf = build_pe(&sections);

        {
            let pe = parse_pe(&buf).expect("should parse");
            assert!(
                pe.virtual_sections()
                    .next()
                    .expect("iterator should yield")
                    .is_ok(),
                "section should be in-bounds before corruption"
            );
        }

        // Corrupt virtual_address to place the section past the end of buf.
        let vaddr_off = SECT_HDR_OFF + offset_of!(SectionHeader, virtual_address);
        let buf_len = buf.len() as u32;
        write_u32(&mut buf, vaddr_off, buf_len);

        let pe = parse_pe(&buf).expect("should still parse");
        let result = pe.virtual_sections().next().expect("iterator should yield");
        assert!(matches!(result, Err(PeError::Truncated)));
    }

    // ── PeRef::relocate_into ──────────────────────────────────────────────

    #[test]
    fn test_relocate_into_success() {
        let raw = b"PAYLOAD!".to_vec();
        let sections = [(*b".text\0\0\0", 0x1000u32, raw.clone())];
        let buf = build_pe(&sections);
        let pe = parse_pe(&buf).expect("should parse");

        let size_of_image = pe.nt_hdrs.optional_header.size_of_image as usize;
        // Pre-fill with a non-zero sentinel so the zero-fill assertion is
        // meaningful: if relocate_into skips the fill, the test will fail.
        let mut pages = vec![0xAAu8; size_of_image];
        pe.relocate_into(&mut pages)
            .expect("relocate_into should succeed");

        // The section data should appear at virtual_address 0x1000
        assert_eq!(&pages[0x1000..0x1000 + raw.len()], raw.as_slice());

        // Bytes beyond raw data but within the section-aligned region must be
        // zero-filled by relocate_into.  section_alignment is 4096, so the
        // padded region runs from 0x1000 + raw.len() to 0x2000.
        let section_alignment = pe.nt_hdrs.optional_header.section_alignment as usize;
        let virt_end = 0x1000 + section_alignment;
        assert!(pages[0x1000 + raw.len()..virt_end].iter().all(|&b| b == 0));
    }

    #[test]
    fn test_relocate_into_output_too_small() {
        let sections = [(*b".text\0\0\0", 0x1000u32, vec![0u8; 8])];
        let buf = build_pe(&sections);
        let pe = parse_pe(&buf).expect("should parse");

        // Provide a destination that is too small to hold the headers
        let mut pages = vec![0u8; 1];
        let result = pe.relocate_into(&mut pages);
        assert!(matches!(result, Err(PeError::Truncated)));
    }

    #[test]
    fn test_relocate_into_with_relocations_errors() {
        let sections = [(*b".text\0\0\0", 0x1000u32, vec![0u8; 8])];
        let mut buf = build_pe(&sections);
        let pe_check = parse_pe(&buf).expect("should parse initially");
        let size_of_image = pe_check.nt_hdrs.optional_header.size_of_image as usize;

        // Set pointer_to_relocations != 0 in the first section header
        let reloc_off = SECT_HDR_OFF + offset_of!(SectionHeader, pointer_to_relocations);
        write_u32(&mut buf, reloc_off, 0x400);

        let pe = parse_pe(&buf).expect("should still parse");
        let mut pages = vec![0u8; size_of_image];
        let result = pe.relocate_into(&mut pages);
        assert!(matches!(result, Err(PeError::RelocationsNotYetSupported)));
    }
}

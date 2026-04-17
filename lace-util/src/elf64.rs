// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! Minimal ELF64 parser
//!
//! Parses ELF64 headers and iterates over program headers.

use zerocopy::{FromBytes, Immutable, KnownLayout};

const ELF_MAGIC: [u8; 4] = *b"\x7fELF";
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;

/// ELF program header type: loadable segment.
pub const PT_LOAD: u32 = 1;

#[repr(C)]
#[derive(FromBytes, Immutable, KnownLayout)]
struct Elf64Header {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

/// A parsed ELF64 program header.
#[derive(Debug, Clone, Copy)]
pub struct Elf64Phdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

#[repr(C)]
#[derive(FromBytes, Immutable, KnownLayout)]
struct RawElf64Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

/// A parsed ELF64 binary.
#[derive(Debug)]
pub struct Elf64<'a> {
    data: &'a [u8],
    entry: u64,
    phoff: usize,
    phentsize: usize,
    phnum: usize,
}

impl<'a> Elf64<'a> {
    /// Parse an ELF64 binary from a byte slice.
    pub fn parse(data: &'a [u8]) -> Result<Self, &'static str> {
        let (ehdr, _) = Elf64Header::read_from_prefix(data).map_err(|_| "ELF header too small")?;

        if ehdr.e_ident[0..4] != ELF_MAGIC {
            return Err("bad ELF magic");
        }
        if ehdr.e_ident[4] != ELFCLASS64 {
            return Err("not ELF64");
        }
        if ehdr.e_ident[5] != ELFDATA2LSB {
            return Err("not little-endian");
        }

        let phentsize = ehdr.e_phentsize as usize;
        if phentsize < size_of::<RawElf64Phdr>() {
            return Err("phentsize too small");
        }

        Ok(Self {
            data,
            entry: ehdr.e_entry,
            phoff: ehdr.e_phoff as usize,
            phentsize,
            phnum: ehdr.e_phnum as usize,
        })
    }

    /// Entry point address.
    pub fn entry(&self) -> u64 {
        self.entry
    }

    /// Iterate over program headers, calling `f` for each.
    ///
    /// The callback returns `true` to continue, `false` to stop.
    pub fn for_each_phdr(&self, mut f: impl FnMut(&Elf64Phdr) -> bool) -> Result<(), &'static str> {
        for i in 0..self.phnum {
            let off = self.phoff + i * self.phentsize;
            let (raw, _) =
                RawElf64Phdr::read_from_prefix(self.data.get(off..).ok_or("phdr out of bounds")?)
                    .map_err(|_| "phdr parse error")?;

            let phdr = Elf64Phdr {
                p_type: raw.p_type,
                p_flags: raw.p_flags,
                p_offset: raw.p_offset,
                p_vaddr: raw.p_vaddr,
                p_paddr: raw.p_paddr,
                p_filesz: raw.p_filesz,
                p_memsz: raw.p_memsz,
                p_align: raw.p_align,
            };

            if !f(&phdr) {
                break;
            }
        }
        Ok(())
    }

    /// Get the file data for a program header's file portion.
    pub fn segment_data(&self, phdr: &Elf64Phdr) -> Result<&'a [u8], &'static str> {
        let off = phdr.p_offset as usize;
        let len = phdr.p_filesz as usize;
        self.data
            .get(off..off + len)
            .ok_or("segment data out of bounds")
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// Build a minimal ELF64 binary with the given PT_LOAD segments.
    /// Each segment is (p_vaddr, p_paddr, data).
    fn build_elf64(entry: u64, segments: &[(u64, u64, &[u8])]) -> alloc::vec::Vec<u8> {
        let ehdr_size = size_of::<Elf64Header>();
        let phdr_size = size_of::<RawElf64Phdr>();
        let phoff = ehdr_size;
        let data_start = ehdr_size + segments.len() * phdr_size;

        let mut buf = alloc::vec![0u8; data_start];

        // ELF header
        buf[0..4].copy_from_slice(b"\x7fELF");
        buf[4] = 2; // ELFCLASS64
        buf[5] = 1; // ELFDATA2LSB
        buf[6] = 1; // EV_CURRENT
        // e_type at 16
        buf[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
        // e_machine at 18
        buf[18..20].copy_from_slice(&62u16.to_le_bytes()); // EM_X86_64
        // e_version at 20
        buf[20..24].copy_from_slice(&1u32.to_le_bytes());
        // e_entry at 24
        buf[24..32].copy_from_slice(&entry.to_le_bytes());
        // e_phoff at 32
        buf[32..40].copy_from_slice(&(phoff as u64).to_le_bytes());
        // e_ehsize at 52
        buf[52..54].copy_from_slice(&(ehdr_size as u16).to_le_bytes());
        // e_phentsize at 54
        buf[54..56].copy_from_slice(&(phdr_size as u16).to_le_bytes());
        // e_phnum at 56
        buf[56..58].copy_from_slice(&(segments.len() as u16).to_le_bytes());

        // Program headers and data
        for (i, &(vaddr, paddr, data)) in segments.iter().enumerate() {
            let file_offset = buf.len();
            buf.extend_from_slice(data);

            let phdr_off = phoff + i * phdr_size;
            // p_type = PT_LOAD (1)
            buf[phdr_off..phdr_off + 4].copy_from_slice(&1u32.to_le_bytes());
            // p_offset
            buf[phdr_off + 8..phdr_off + 16].copy_from_slice(&(file_offset as u64).to_le_bytes());
            // p_vaddr
            buf[phdr_off + 16..phdr_off + 24].copy_from_slice(&vaddr.to_le_bytes());
            // p_paddr
            buf[phdr_off + 24..phdr_off + 32].copy_from_slice(&paddr.to_le_bytes());
            // p_filesz
            buf[phdr_off + 32..phdr_off + 40].copy_from_slice(&(data.len() as u64).to_le_bytes());
            // p_memsz (same as filesz for simplicity)
            buf[phdr_off + 40..phdr_off + 48].copy_from_slice(&(data.len() as u64).to_le_bytes());
        }

        buf
    }

    #[test]
    fn test_parse_valid_elf() {
        let elf_data = build_elf64(0x100000, &[(0x100000, 0x100000, b"hello")]);
        let elf = Elf64::parse(&elf_data).unwrap();
        assert_eq!(elf.entry(), 0x100000);
    }

    #[test]
    fn test_bad_magic() {
        let mut data = build_elf64(0, &[]);
        data[0] = 0; // corrupt magic
        assert!(Elf64::parse(&data).is_err());
    }

    #[test]
    fn test_not_elf64() {
        let mut data = build_elf64(0, &[]);
        data[4] = 1; // ELFCLASS32
        assert_eq!(Elf64::parse(&data).unwrap_err(), "not ELF64");
    }

    #[test]
    fn test_not_little_endian() {
        let mut data = build_elf64(0, &[]);
        data[5] = 2; // ELFDATA2MSB
        assert_eq!(Elf64::parse(&data).unwrap_err(), "not little-endian");
    }

    #[test]
    fn test_program_headers() {
        let seg1 = b"segment one data";
        let seg2 = b"two";
        let elf_data = build_elf64(
            0x200000,
            &[(0x100000, 0x100000, seg1), (0x200000, 0x200000, seg2)],
        );
        let elf = Elf64::parse(&elf_data).unwrap();

        let mut phdrs = alloc::vec::Vec::new();
        elf.for_each_phdr(|phdr| {
            phdrs.push(*phdr);
            true
        })
        .unwrap();

        assert_eq!(phdrs.len(), 2);
        assert_eq!(phdrs[0].p_type, PT_LOAD);
        assert_eq!(phdrs[0].p_vaddr, 0x100000);
        assert_eq!(phdrs[0].p_filesz, seg1.len() as u64);
        assert_eq!(phdrs[1].p_vaddr, 0x200000);

        // Check segment data
        let data = elf.segment_data(&phdrs[0]).unwrap();
        assert_eq!(data, seg1);
    }

    #[test]
    fn test_too_small() {
        assert!(Elf64::parse(&[0; 10]).is_err());
    }
}

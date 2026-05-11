// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

use clap::Parser;
use lace_util::chid_mapping::*;
use lace_util::peimage::*;
use lace_util::*;
use pewrap::cli::Args;
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, fs::DirEntry, io, mem::offset_of, path::Path, process};
use zerocopy::IntoBytes;

fn main() {
    let args = Args::parse();

    // Parse stub PE image
    let data = match std::fs::read(&args.stub) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{}: {}", args.stub.display(), e);
            process::exit(1);
        }
    };
    let (_pe, mut bld) = match parse_pe(&data).and_then(|pe| {
        let bld = PeRebuilder::from_ref(&pe)?;
        Ok((pe, bld))
    }) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{}: {}", args.stub.display(), e);
            process::exit(1);
        }
    };

    /*
        println!("{:#x?}", pe.dos_hdr);
        println!("{:#x?}", pe.nt_hdrs);

        println!("PE Sections");
        for sect in pe.sect_hdrs.iter() {
            println!(
                "  {:8} Raw data {:08x} raw size {:08x} VA {:08x} Virt size {:08x} Characteristics {:8x}",
                str::from_utf8(sect.name()).unwrap(),
                sect.pointer_to_raw_data,
                sect.size_of_raw_data,
                sect.virtual_address,
                sect.virtual_size,
                sect.characteristics
            );
        }
    */

    // Add sections from files
    let mut file_sections = vec![
        (".linux", args.linux),
        (".initrd", args.initrd),
        (".sbat", args.sbat),
    ];
    for dtb_path in args.dtbauto.iter() {
        file_sections.push((".dtbauto", Some(dtb_path.clone())));
    }

    for (name, path) in file_sections.iter() {
        let Some(path) = path else {
            continue;
        };
        let d = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("{}: {}", path.display(), e);
                process::exit(1);
            }
        };

        // Section specific checks
        match *name {
            ".linux" => {
                // Validate Linux kernel PE
                match lace_util::peimage::parse_pe(&d) {
                    Ok(pe) => {
                        // Check for LoadFile2 initrd support
                        if pe.nt_hdrs.optional_header.major_image_version < 1 {
                            eprintln!(
                                "{}: Linux kernel PE image does not support LoadFile2 initrd",
                                path.display()
                            );
                            process::exit(1);
                        }

                        // Set stubble PE major version to 1 to indicate LoadFile2 initrd support
                        bld.nt_hdrs.optional_header.major_image_version = 1;

                        // Set stubble NX_COMPAT based on kernel NX_COMPAT
                        let kernel_nx_compat = (pe.nt_hdrs.optional_header.dll_characteristics
                            & peimage::DLLCHARACTERISTICS_NX_COMPAT)
                            != 0;
                        if kernel_nx_compat {
                            bld.nt_hdrs.optional_header.dll_characteristics |=
                                peimage::DLLCHARACTERISTICS_NX_COMPAT;
                        } else {
                            bld.nt_hdrs.optional_header.dll_characteristics &=
                                !peimage::DLLCHARACTERISTICS_NX_COMPAT;
                        }
                    }
                    Err(e) => {
                        eprintln!("{}: invalid Linux kernel PE image: {}", path.display(), e);
                        process::exit(1);
                    }
                }
            }
            ".dtbauto" => {
                // Validate DTB files using the fdt crate
                if let Err(e) = fdt::Fdt::new(&d) {
                    eprintln!("{}: invalid device tree blob: {}", path.display(), e);
                    process::exit(1);
                }
            }
            _ => {}
        }

        bld.add_section(name, d, SCN_CNT_INITIALIZED_DATA | SCN_MEM_READ);
    }

    // Add sections from command line
    for (name, data) in [(".cmdline", args.cmdline)] {
        let Some(data) = data else {
            continue;
        };
        bld.add_section(
            name,
            data.into_bytes(),
            SCN_CNT_INITIALIZED_DATA | SCN_MEM_READ,
        );
    }

    // Add section from HWIDs
    if let Some(hwids_dir) = args.hwids {
        // Read HWID JSON files
        let hwid_jsons = match read_hwids_dir(&hwids_dir) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("{}: {}", hwids_dir.display(), e);
                process::exit(1);
            }
        };
        // Generate CHID mappings
        let mut chid_mappings = Vec::new();
        for hwid_json in hwid_jsons.iter() {
            if let Err(e) = hwid_json.generate_chid_mappings(&mut chid_mappings) {
                eprintln!("{}: {}", hwids_dir.display(), e);
                process::exit(1);
            }
        }
        // Serialize CHID mappings
        let mut hwids_section_data = Vec::new();
        if let Err(e) = serialize_chid_mappings(&mut hwids_section_data, &chid_mappings) {
            eprintln!("{}: {}", hwids_dir.display(), e);
            process::exit(1);
        }
        // Add HWIDs section
        bld.add_section(
            ".hwids",
            hwids_section_data,
            SCN_CNT_INITIALIZED_DATA | SCN_MEM_READ,
        );
    }

    // If we are called with --post-process-for-ukify, set PE major version to 1 for LoadFile2
    // We set this above only if the Linux kernel supports it, but ukify never sets it itself,
    // so we need to set it here unconditionally.
    if args.post_process_for_ukify {
        bld.nt_hdrs.optional_header.major_image_version = 1;
    }

    // Calculate section offsets
    if let Err(e) = bld.fixup_offsets(args.post_process_for_ukify) {
        eprintln!("{}: {}", args.output.display(), e);
        process::exit(1);
    }

    // Write output file
    if let Err(e) = std::fs::File::create(&args.output).map(|x| bld.write_pe(x)) {
        eprintln!("{}: {}", args.output.display(), e);
        process::exit(1);
    }
}

/// Representation of the JSON HWID file
#[derive(Debug, Serialize, Deserialize)]
struct HwidJson {
    #[serde(rename = "type")]
    type_: String,
    name: Option<String>,
    compatible: Option<String>,
    fwid: Option<String>,
    hwids: Vec<String>,
}

/// Errors that can occur when generating CHID mappings from HWID JSON
#[derive(Clone, Debug, Display)]
enum GenerateChidMappingsError {
    #[display("unknown HWID type: {}")]
    UnknownType(String),
    #[display("{}")]
    GuidParseError(GuidParseError),
}

impl From<GuidParseError> for GenerateChidMappingsError {
    fn from(e: GuidParseError) -> Self {
        GenerateChidMappingsError::GuidParseError(e)
    }
}

impl HwidJson {
    fn generate_chid_mappings<'s>(
        &'s self,
        v: &mut Vec<ChidMapping<'s>>,
    ) -> Result<(), GenerateChidMappingsError> {
        for hwid in self.hwids.iter() {
            let guid = Guid::try_from_str(hwid)?;
            match self.type_.as_str() {
                "devicetree" => {
                    v.push(ChidMapping::DeviceTree {
                        chid: guid,
                        name: self.name.as_deref(),
                        compatible: self.compatible.as_deref(),
                    });
                }
                "uefi-fw" => {
                    v.push(ChidMapping::UefiFw {
                        chid: guid,
                        name: self.name.as_deref(),
                        fwid: self.fwid.as_deref(),
                    });
                }
                _type => {
                    return Err(GenerateChidMappingsError::UnknownType(_type.to_owned()));
                }
            }
        }
        Ok(())
    }
}

/// Read all HWIDs from JSON files in the given directory and its subdirectories
fn read_hwids_dir(path: &Path) -> io::Result<Vec<HwidJson>> {
    let mut hwids = Vec::new();
    for entry in DirWalker::new(path)? {
        let entry = entry?;
        if entry.path().is_file()
            && entry.path().extension().and_then(|s| s.to_str()) == Some("json")
        {
            let file = std::fs::File::open(entry.path())?;
            hwids.push(serde_json::from_reader(file)?);
        }
    }
    Ok(hwids)
}

/// Recursive directory tree iterator
struct DirWalker {
    queue: VecDeque<DirEntry>,
}

impl DirWalker {
    fn new(path: &Path) -> io::Result<Self> {
        let mut queue = VecDeque::new();
        for entry in std::fs::read_dir(path)? {
            queue.push_back(entry?);
        }
        Ok(DirWalker { queue })
    }
}

impl Iterator for DirWalker {
    type Item = io::Result<DirEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        self.queue.pop_front().map(|entry| {
            if entry.path().is_dir() {
                for entry in std::fs::read_dir(entry.path())? {
                    self.queue.push_back(entry?);
                }
            }
            Ok(entry)
        })
    }
}

struct PeRebuilder<'s> {
    dos_hdr: DosHeader,
    dos_data: &'s [u8],
    nt_hdrs: NtHeaders64,
    nt_data: &'s [u8],
    sections: Vec<(SectionHeader, Vec<u8>)>,
}

#[derive(Clone, Copy, Debug, Display)]
enum PeRebuildError {
    #[display("PE headers exceed maximum allowed size")]
    HeadersTooLarge,
}

impl<'s> PeRebuilder<'s> {
    fn from_ref(r: &PeRef<'s>) -> Result<Self, PeError> {
        let mut sections = Vec::new();
        for result in r.raw_sections() {
            let (shdr, data) = result?;
            sections.push((shdr, data.to_owned()));
        }
        Ok(PeRebuilder {
            dos_hdr: r.dos_hdr.clone(),
            dos_data: r.dos_data,
            nt_hdrs: r.nt_hdrs.clone(),
            nt_data: r.nt_data,
            sections,
        })
    }

    fn add_section(&mut self, name: &str, data: Vec<u8>, characteristics: u32) {
        // Truncate section namearr to 8 bytes and pad with 0s
        let mut namearr = [0u8; 8];
        let namelen = std::cmp::min(namearr.len(), name.len());
        namearr[..namelen].copy_from_slice(&name.as_bytes()[..namelen]);

        // Figure out the first available VA we can freely put a section of any length
        let first_avail_va = self
            .sections
            .iter()
            .map(|(shdr, _)| shdr.virtual_address + shdr.virtual_size)
            .max()
            .unwrap_or(self.nt_hdrs.optional_header.size_of_headers);

        // Build section header
        let shdr = SectionHeader {
            name: namearr,
            // Virtual size is not aligned as per PE spec,
            // and the PE loader is expected to handle that
            virtual_size: data.len() as u32,
            // Virtual address is aligned as per PE spec
            virtual_address: align_up!(
                first_avail_va,
                self.nt_hdrs.optional_header.section_alignment,
            ),
            // Counter-intuitively, raw size is aligned as per PE spec
            size_of_raw_data: align_up!(
                data.len() as u32,
                self.nt_hdrs.optional_header.file_alignment,
            ),
            // This will be filled in later when building the final image
            pointer_to_raw_data: 0,
            // These are only relevant for object files
            pointer_to_relocations: 0,
            pointer_to_linenumbers: 0,
            number_of_relocations: 0,
            number_of_linenumbers: 0,
            // Characteristics as specified
            characteristics,
        };

        self.sections.push((shdr, data));
    }

    fn fixup_offsets(&mut self, maximize_header_space: bool) -> Result<(), PeRebuildError> {
        self.nt_hdrs.file_header.number_of_sections = self.sections.len() as u16;
        self.nt_hdrs.optional_header.size_of_code = 0;
        self.nt_hdrs.optional_header.size_of_initialized_data = 0;
        self.nt_hdrs.optional_header.size_of_uninitialized_data = 0;
        let mut unaligned_size_of_headers = self.dos_hdr.e_lfanew
            + offset_of!(NtHeaders64, optional_header) as u32
            + self.nt_hdrs.file_header.size_of_optional_header as u32
            + self.nt_hdrs.file_header.number_of_sections as u32
                * size_of::<SectionHeader>() as u32;
        if maximize_header_space {
            // Increase SizeOfHeaders to maximum possible to allow adding more sections later
            let min_section_virtual_address = self
                .sections
                .iter()
                .map(|(shdr, _)| shdr.virtual_address)
                .min()
                .unwrap_or(unaligned_size_of_headers);
            if min_section_virtual_address > unaligned_size_of_headers {
                // Setting this unaligned value to the clearly section aligned VA is fine,
                // because section alignment is a multiple of file alignment, and the below
                // align_up! calls will fix do nothing.
                unaligned_size_of_headers = min_section_virtual_address;
            }
        }
        // Loaded image size is the maximum aligned end RVA across headers and
        // all section mappings.
        self.nt_hdrs.optional_header.size_of_image = align_up!(
            unaligned_size_of_headers,
            self.nt_hdrs.optional_header.section_alignment
        );
        // PE spec says size of headers is rounded to the file alignment
        self.nt_hdrs.optional_header.size_of_headers = align_up!(
            unaligned_size_of_headers,
            self.nt_hdrs.optional_header.file_alignment
        );

        let mut off = self.nt_hdrs.optional_header.size_of_headers;
        for (shdr, _) in self.sections.iter_mut() {
            // See if the headers fit before the virtual address of any section.
            // Unfortunately this is an unfixable necessity because the PE images we
            // operate on only have base relocations which means we cannot move the
            // first section to start further from the image base in virtual memory.
            // (Base relocations only allow rebasing the entire image.)
            // Thankfully section alignment is at least 4K in real PEs, and
            // the size of the headers is usually around 500 bytes at most, so we are
            // not going to run out of space unless we add a crazy number of new sections.
            if self.nt_hdrs.optional_header.size_of_headers > shdr.virtual_address {
                return Err(PeRebuildError::HeadersTooLarge);
            }

            // Unlike in virtual space, we can move everything around in file space,
            // so we do not care about there being enough space in the file after the
            // headers to add additionally section headers, instead we dynamically recalculate
            // all raw data offsets, and re-write the whole file.
            shdr.pointer_to_raw_data = off;
            // For sections we added, this is already aligned, but the PE spec
            // mandates this being aligned for all sections, so let's just fix up
            // after bad linkers too.
            shdr.size_of_raw_data = align_up!(
                shdr.size_of_raw_data,
                self.nt_hdrs.optional_header.file_alignment
            );
            off += shdr.size_of_raw_data;

            // Update the various size fields in the optional header
            if (shdr.characteristics & SCN_CNT_CODE) > 0 {
                self.nt_hdrs.optional_header.size_of_code += shdr.size_of_raw_data;
            }
            if (shdr.characteristics & SCN_CNT_INITIALIZED_DATA) > 0 {
                self.nt_hdrs.optional_header.size_of_initialized_data += shdr.size_of_raw_data;
            }
            if (shdr.characteristics & SCN_CNT_UNINITIALIZED_DATA) > 0 {
                self.nt_hdrs.optional_header.size_of_uninitialized_data += shdr.size_of_raw_data;
            }
            let mapped_size = if shdr.virtual_size == 0 {
                shdr.size_of_raw_data
            } else {
                shdr.virtual_size
            };
            let section_end = align_up!(
                shdr.virtual_address.saturating_add(mapped_size),
                self.nt_hdrs.optional_header.section_alignment
            );
            self.nt_hdrs.optional_header.size_of_image =
                self.nt_hdrs.optional_header.size_of_image.max(section_end);
        }

        Ok(())
    }

    fn write_pe<W: io::Write>(&self, mut w: W) -> io::Result<()> {
        let mut off = 0;

        // Write headers
        off += w.write(self.dos_hdr.as_bytes())?;
        off += w.write(self.dos_data)?;
        off += w.write(self.nt_hdrs.as_bytes())?;
        off += w.write(self.nt_data)?;
        for (shdr, _) in self.sections.iter() {
            off += w.write(shdr.as_bytes())?;
        }
        // Pad headers
        off += write_zeros(
            &mut w,
            self.nt_hdrs.optional_header.size_of_headers as usize - off,
        )?;

        for (shdr, sdata) in self.sections.iter() {
            assert_eq!(shdr.pointer_to_raw_data as usize, off);
            // Write section
            off += w.write(sdata)?;
            // Pad section
            off += write_zeros(&mut w, shdr.size_of_raw_data as usize - sdata.len())?;
        }

        Ok(())
    }
}

fn write_zeros<W: io::Write>(mut w: W, n: usize) -> io::Result<usize> {
    let mut cnt = 0;
    for _ in 0..n {
        cnt += w.write(&[0])?;
    }
    Ok(cnt)
}

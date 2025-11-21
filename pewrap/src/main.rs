// Allow manual_div_ceil for the align_up! macro usage
#![allow(clippy::manual_div_ceil)]

use std::{ffi::OsString, fmt::Display, io, mem::offset_of, process};

use clap::Parser;
use lace_util::peimage::*;
use lace_util::*;
use zerocopy::IntoBytes;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to input stub PE image
    #[arg(short, long)]
    stub: OsString,

    /// Path to output PE image
    #[arg(short, long)]
    output: OsString,

    /// Path to linux kernel image to add
    #[arg(short, long)]
    linux: Option<OsString>,

    /// Path to initrd image to add
    #[arg(short, long)]
    initrd: Option<OsString>,

    /// Kernel command line to add
    #[arg(short, long)]
    cmdline: Option<OsString>,
}

fn main() {
    let args = Args::parse();

    // Parse stub PE image
    let data = match std::fs::read(&args.stub) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{}: {}", args.stub.to_string_lossy(), e);
            process::exit(1);
        }
    };
    let pe = match parse_pe(&data) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{}: {}", args.stub.to_string_lossy(), e);
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

    let mut bld = PeRebuilder::from_ref(&pe);

    // Add sections
    for (name, path) in [(".linux", args.linux), (".initrd", args.initrd)] {
        let Some(path) = path else {
            continue;
        };
        let d = match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("{}: {}", path.to_string_lossy(), e);
                process::exit(1);
            }
        };
        bld.add_section(name, d, SCN_CNT_INITIALIZED_DATA | SCN_MEM_READ);
    }
    for (name, data) in [(".cmdline", args.cmdline)] {
        let Some(data) = data else {
            continue;
        };
        bld.add_section(
            name,
            data.into_encoded_bytes(),
            SCN_CNT_INITIALIZED_DATA | SCN_MEM_READ,
        );
    }

    // Calculate section offsets
    if let Err(e) = bld.fixup_offsets() {
        eprintln!("{}: {}", args.output.to_string_lossy(), e);
        process::exit(1);
    }

    // Write output file
    if let Err(e) = std::fs::File::create(&args.output).map(|x| bld.write_pe(x)) {
        eprintln!("{}: {}", args.output.to_string_lossy(), e);
        process::exit(1);
    }
}

struct PeRebuilder<'s> {
    dos_hdr: DosHeader,
    dos_data: &'s [u8],
    nt_hdrs: NtHeaders64,
    nt_data: &'s [u8],
    sections: Vec<(SectionHeader, Vec<u8>)>,
}

#[derive(Clone, Copy, Debug)]
enum PeRebuildError {
    HeadersTooLarge,
}

impl Display for PeRebuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PeRebuildError::HeadersTooLarge => write!(f, "PE headers exceed maximum allowed size"),
        }
    }
}

impl<'s> PeRebuilder<'s> {
    fn from_ref(r: &PeRef<'s>) -> Self {
        let mut sections = Vec::new();
        for shdr in r.sect_hdrs.iter().cloned() {
            let i = shdr.pointer_to_raw_data as usize;
            let j = i + shdr.size_of_raw_data as usize;
            sections.push((shdr, r.data[i..j].to_owned()));
        }
        PeRebuilder {
            dos_hdr: r.dos_hdr.clone(),
            dos_data: r.dos_data,
            nt_hdrs: r.nt_hdrs.clone(),
            nt_data: r.nt_data,
            sections,
        }
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

    fn fixup_offsets(&mut self) -> Result<(), PeRebuildError> {
        self.nt_hdrs.file_header.number_of_sections = self.sections.len() as u16;
        self.nt_hdrs.optional_header.size_of_code = 0;
        self.nt_hdrs.optional_header.size_of_initialized_data = 0;
        self.nt_hdrs.optional_header.size_of_uninitialized_data = 0;
        let raw_size_of_headers = self.dos_hdr.e_lfanew
            + offset_of!(NtHeaders64, optional_header) as u32
            + self.nt_hdrs.file_header.size_of_optional_header as u32
            + self.nt_hdrs.file_header.number_of_sections as u32
                * size_of::<SectionHeader>() as u32;
        // Loaded image size starts with the size of headers rounded to section alignment
        self.nt_hdrs.optional_header.size_of_image = align_up!(
            raw_size_of_headers,
            self.nt_hdrs.optional_header.section_alignment
        );
        // PE spec says size of headers is rounded to the file alignment
        self.nt_hdrs.optional_header.size_of_headers = align_up!(
            raw_size_of_headers,
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
            self.nt_hdrs.optional_header.size_of_image += align_up!(
                shdr.virtual_size,
                self.nt_hdrs.optional_header.section_alignment
            );
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

// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! flashedit: Build and manipulate CBFS flash images

use clap::{Parser, Subcommand};
use lace_util::cbfs::*;
use std::fs;
use std::path::PathBuf;
use zerocopy::byteorder::U32;
use zerocopy::{FromBytes, IntoBytes};

/// Write a CBFS file header + name at `entry_start` in `rom`, with the
/// header's `offset` field set to `data_offset` (distance from header
/// start to the file's data).
fn write_file_header(
    rom: &mut [u8],
    entry_start: usize,
    name: &str,
    type_: u32,
    data_len: usize,
    data_offset: usize,
) {
    let hdr = CbfsFileHeader {
        magic: CBFS_FILE_MAGIC,
        len: U32::new(data_len as u32),
        type_: U32::new(type_),
        attributes_offset: U32::new(0),
        offset: U32::new(data_offset as u32),
    };
    rom[entry_start..entry_start + CBFS_FILE_HEADER_SIZE].copy_from_slice(hdr.as_bytes());
    let name_start = entry_start + CBFS_FILE_HEADER_SIZE;
    rom[name_start..name_start + name.len()].copy_from_slice(name.as_bytes());
    rom[name_start + name.len()] = 0;
}

/// Place a file at a given data-content offset within `rom`, following
/// cbfstool's `cbfs_add_entry_at` in `coreboot/util/cbfstool/cbfs_image.c`.
///
/// The file header goes at `content_offset - header_size`, aligned
/// down to `CBFS_ALIGNMENT`. Any gap between `cursor` and the header
/// start is filled with a `CBFS_TYPE_NULL` entry. Returns the cursor
/// just past the newly placed entry.
fn add_entry_at(
    rom: &mut [u8],
    cursor: usize,
    content_offset: usize,
    name: &str,
    type_: u32,
    data: &[u8],
) -> usize {
    let header_size = cbfs_file_header_total_size(name);
    assert!(
        content_offset >= cursor + header_size,
        "no room for '{}' header at content offset {:#x}",
        name,
        content_offset
    );
    assert!(
        content_offset + data.len() <= rom.len(),
        "'{}' data ({} bytes) overflows ROM at content offset {:#x}",
        name,
        data.len(),
        content_offset
    );
    // Align the header start down so every CBFS entry starts on a
    // `CBFS_ALIGNMENT` boundary; the gap between the aligned start and
    // the data is absorbed by the header's `offset` field.
    let entry_start = (content_offset - header_size) & !(CBFS_ALIGNMENT - 1);
    let data_offset = content_offset - entry_start;

    // Fill the gap from the previous cursor to this entry with a null
    // entry (or leave 0xFF if smaller than the minimum entry header).
    let gap = entry_start - cursor;
    let null_hdr = cbfs_file_header_total_size("");
    if gap >= null_hdr {
        write_file_header(rom, cursor, "", CBFS_TYPE_NULL, gap - null_hdr, null_hdr);
    }

    // Write the file header + name + data.
    write_file_header(rom, entry_start, name, type_, data.len(), data_offset);
    rom[content_offset..content_offset + data.len()].copy_from_slice(data);

    cbfs_align_up(entry_start + data_offset + data.len())
}

#[derive(Parser)]
#[command(name = "flashedit", about = "Build and manipulate CBFS flash images")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new CBFS flash image
    Create {
        /// Output file path
        #[arg(short, long)]
        output: PathBuf,
        /// ROM size in bytes (e.g. 65536 for 64KB, or use suffixes like 64K, 1M)
        #[arg(short, long, value_parser = parse_size)]
        size: usize,
        /// Bootblock flat binary file
        #[arg(short, long)]
        bootblock: PathBuf,
        /// Files to add: name=path pairs (e.g. firmware=firmware.elf)
        #[arg(short, long = "file", value_parser = parse_file_arg)]
        files: Vec<(String, PathBuf)>,
    },
    /// List files in a CBFS image
    List {
        /// Input CBFS image file
        #[arg(short, long)]
        input: PathBuf,
    },
}

fn parse_size(s: &str) -> Result<usize, String> {
    let s = s.trim();
    if let Some(prefix) = s.strip_suffix('K') {
        prefix
            .trim()
            .parse::<usize>()
            .map(|n| n * 1024)
            .map_err(|e| e.to_string())
    } else if let Some(prefix) = s.strip_suffix('M') {
        prefix
            .trim()
            .parse::<usize>()
            .map(|n| n * 1024 * 1024)
            .map_err(|e| e.to_string())
    } else {
        s.parse::<usize>().map_err(|e| e.to_string())
    }
}

fn parse_file_arg(s: &str) -> Result<(String, PathBuf), String> {
    let (name, path) = s.split_once('=').ok_or("expected name=path")?;
    Ok((name.to_string(), PathBuf::from(path)))
}

fn cmd_create(output: PathBuf, size: usize, bootblock: PathBuf, files: Vec<(String, PathBuf)>) {
    // ROM size is stored in u32 fields of the CBFS header and used to
    // compute `0x1_0000_0000 - size` as a 32-bit base; reject anything
    // outside the supported range up front.
    if size == 0 || size > u32::MAX as usize {
        panic!("ROM size ({}) must be in (0, 2^32]", size);
    }

    let bb_data = fs::read(&bootblock)
        .unwrap_or_else(|e| panic!("Failed to read bootblock {:?}: {}", bootblock, e));

    if bb_data.len() > size {
        panic!(
            "Bootblock ({} bytes) exceeds ROM size ({} bytes)",
            bb_data.len(),
            size
        );
    }

    // Last 4 bytes of ROM store the CBFS master-header pointer and
    // overlap the bootblock's reset-vector padding region, so the
    // bootblock must cover them.
    if bb_data.len() < 4 {
        panic!(
            "Bootblock ({} bytes) is too short to host the CBFS header pointer",
            bb_data.len()
        );
    }

    // Start with empty ROM (0xFF)
    let mut rom = vec![0xFFu8; size];

    // Master header lives in its own CBFS file at the start of the ROM.
    let master_header = CbfsHeader {
        magic: U32::new(CBFS_HEADER_MAGIC),
        version: U32::new(CBFS_HEADER_VERSION2),
        romsize: U32::new(size as u32),
        bootblocksize: U32::new(bb_data.len() as u32),
        align: U32::new(CBFS_ALIGNMENT as u32),
        offset: U32::new(0),
        architecture: U32::new(CBFS_ARCHITECTURE_X86),
        _pad: U32::new(0),
    };
    let master_header_bytes = master_header.as_bytes();
    let mh_name = "cbfs_master_header";
    let mh_content_offset = cbfs_file_header_total_size(mh_name);
    let mut cursor = add_entry_at(
        &mut rom,
        0,
        mh_content_offset,
        mh_name,
        CBFS_TYPE_RAW,
        master_header_bytes,
    );
    eprintln!(
        "  {:40} offset={:#010x} size={}",
        mh_name,
        mh_content_offset,
        master_header_bytes.len()
    );

    // User files, packed after the master header.
    for (name, path) in &files {
        let data = fs::read(path).unwrap_or_else(|e| panic!("Failed to read {:?}: {}", path, e));
        let header_size = cbfs_file_header_total_size(name);
        let content_offset = cbfs_align_up(cursor + header_size);
        eprintln!(
            "  {:40} offset={:#010x} size={}",
            name,
            content_offset,
            data.len()
        );
        cursor = add_entry_at(&mut rom, cursor, content_offset, name, CBFS_TYPE_RAW, &data);
    }

    // Bootblock sits at the top of ROM so the x86 reset vector lands
    // at 0xFFFFFFF0. It's tracked as a CBFS entry so cbfstool and
    // other consumers see it; the gap between `cursor` and the
    // bootblock header becomes the trailing null entry.
    let bb_content_offset = size - bb_data.len();
    eprintln!(
        "  {:40} offset={:#010x} size={}",
        "bootblock",
        bb_content_offset,
        bb_data.len()
    );
    add_entry_at(
        &mut rom,
        cursor,
        bb_content_offset,
        "bootblock",
        CBFS_TYPE_BOOTBLOCK,
        &bb_data,
    );

    // Write header pointer at the last 4 bytes of ROM. This overlaps
    // the bootblock's reset-vector padding; the reset vector sits at
    // 0xFFFFFFF0 (last 16 bytes) and only the first 12 bytes carry
    // actual code, so the final 4 bytes belong to this pointer.
    let rom_base: u64 = 0x1_0000_0000u64 - size as u64;
    let header_addr = (rom_base + mh_content_offset as u64) as u32;
    rom[size - 4..size].copy_from_slice(&header_addr.to_le_bytes());

    fs::write(&output, &rom).unwrap_or_else(|e| panic!("Failed to write {:?}: {}", output, e));
    eprintln!("Created CBFS image {:?} ({} bytes)", output, size);
}

fn cmd_list(input: PathBuf) {
    let rom = fs::read(&input).unwrap_or_else(|e| panic!("Failed to read {:?}: {}", input, e));

    let size = rom.len();

    if size < 4 || size > u32::MAX as usize {
        panic!("ROM size ({}) is not a valid CBFS image size", size);
    }

    // Read header pointer from last 4 bytes
    let ptr_bytes: [u8; 4] = rom[size - 4..size].try_into().unwrap();
    let header_addr = u32::from_le_bytes(ptr_bytes);
    let rom_base = 0x1_0000_0000u64 - size as u64;
    let header_offset = (header_addr as u64)
        .checked_sub(rom_base)
        .and_then(|o| usize::try_from(o).ok())
        .unwrap_or_else(|| panic!("Header pointer {:#010x} out of range", header_addr));

    if header_offset + size_of::<CbfsHeader>() > size {
        panic!("Header pointer out of range");
    }

    let (header, _) =
        CbfsHeader::read_from_prefix(&rom[header_offset..]).expect("Failed to parse CBFS header");

    if header.magic.get() != CBFS_HEADER_MAGIC {
        panic!("Invalid CBFS header magic: {:#010x}", header.magic.get());
    }

    println!(
        "CBFS image: {} bytes, bootblock {} bytes, alignment {}",
        header.romsize.get(),
        header.bootblocksize.get(),
        header.align.get()
    );
    println!();

    // Walk file entries all the way to the end of the ROM; the
    // bootblock is the last CBFS entry.
    let entries_start = header.offset.get() as usize;
    let mut cursor = entries_start;

    println!(
        "{:40} {:>10} {:>10} {:>10}",
        "Name", "Offset", "Size", "Type"
    );
    println!("{}", "-".repeat(74));

    while cursor + CBFS_FILE_HEADER_SIZE <= size {
        let (file_hdr, _) = match CbfsFileHeader::read_from_prefix(&rom[cursor..]) {
            Ok(h) => h,
            Err(_) => break,
        };

        if file_hdr.magic != CBFS_FILE_MAGIC {
            break;
        }

        let type_ = file_hdr.type_.get();
        if type_ == CBFS_TYPE_DELETED {
            break;
        }

        let data_offset = file_hdr.offset.get() as usize;
        let data_len = file_hdr.len.get() as usize;

        // Extract filename
        let name_start = cursor + CBFS_FILE_HEADER_SIZE;
        let name_end = cursor + data_offset;
        let name_bytes = &rom[name_start..name_end];
        let name_len = name_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_bytes.len());
        let name = std::str::from_utf8(&name_bytes[..name_len]).unwrap_or("???");

        let type_str = match type_ {
            CBFS_TYPE_NULL => "(empty)",
            CBFS_TYPE_BOOTBLOCK => "bootblock",
            CBFS_TYPE_RAW => "raw",
            _ => "unknown",
        };

        println!(
            "{:40} {:#010x} {:>10} {:>10}",
            name, cursor, data_len, type_str
        );

        // Advance to next entry (aligned)
        let entry_size = cbfs_align_up(data_offset + data_len);
        cursor += entry_size;
    }
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Create {
            output,
            size,
            bootblock,
            files,
        } => {
            cmd_create(output, size, bootblock, files);
        }
        Command::List { input } => {
            cmd_list(input);
        }
    }
}

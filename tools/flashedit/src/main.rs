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

/// Build a CBFS file entry (header + name + padding + data).
fn build_file_entry(name: &str, type_: u32, data: &[u8]) -> Vec<u8> {
    let header_total = cbfs_file_header_total_size(name);
    let total_size = cbfs_align_up(header_total + data.len());
    let mut entry = vec![0u8; total_size];

    let header = CbfsFileHeader {
        magic: CBFS_FILE_MAGIC,
        len: U32::new(data.len() as u32),
        type_: U32::new(type_),
        attributes_offset: U32::new(0),
        offset: U32::new(header_total as u32),
    };

    entry[..CBFS_FILE_HEADER_SIZE].copy_from_slice(header.as_bytes());
    entry[CBFS_FILE_HEADER_SIZE..CBFS_FILE_HEADER_SIZE + name.len()]
        .copy_from_slice(name.as_bytes());
    // null terminator is already 0 from vec init
    entry[header_total..header_total + data.len()].copy_from_slice(data);
    entry
}

/// Build a CBFS empty/null entry to fill remaining space.
fn build_null_entry(capacity: usize) -> Vec<u8> {
    let name = "";
    let header_total = cbfs_file_header_total_size(name);
    if capacity < header_total {
        return vec![0xFF; capacity];
    }
    let data_len = capacity - header_total;
    let mut entry = vec![0xFF; capacity];

    let header = CbfsFileHeader {
        magic: CBFS_FILE_MAGIC,
        len: U32::new(data_len as u32),
        type_: U32::new(CBFS_TYPE_NULL),
        attributes_offset: U32::new(0),
        offset: U32::new(header_total as u32),
    };

    entry[..CBFS_FILE_HEADER_SIZE].copy_from_slice(header.as_bytes());
    // Name is empty, just null terminator (already 0xFF, set to 0)
    entry[CBFS_FILE_HEADER_SIZE] = 0;
    entry
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
        /// Files to add: name=path pairs (e.g. fallback/payload=firmware.elf)
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
    let bb_data = fs::read(&bootblock)
        .unwrap_or_else(|e| panic!("Failed to read bootblock {:?}: {}", bootblock, e));

    if bb_data.len() > size {
        panic!(
            "Bootblock ({} bytes) exceeds ROM size ({} bytes)",
            bb_data.len(),
            size
        );
    }

    // Start with empty ROM (0xFF)
    let mut rom = vec![0xFFu8; size];

    // Place bootblock at the top of ROM
    let bb_offset = size - bb_data.len();
    rom[bb_offset..].copy_from_slice(&bb_data);

    // CBFS file entries start at offset 0
    let mut cursor: usize = 0;

    // Available space for CBFS entries (everything before the bootblock)
    let cbfs_end = bb_offset;

    // First entry: the master header, stored as a CBFS file
    let header = CbfsHeader {
        magic: U32::new(CBFS_HEADER_MAGIC),
        version: U32::new(CBFS_HEADER_VERSION2),
        romsize: U32::new(size as u32),
        bootblocksize: U32::new(bb_data.len() as u32),
        align: U32::new(CBFS_ALIGNMENT as u32),
        offset: U32::new(0),
        architecture: U32::new(CBFS_ARCHITECTURE_X86),
        _pad: U32::new(0),
    };
    let header_entry = build_file_entry("cbfs_master_header", CBFS_TYPE_RAW, header.as_bytes());
    let header_data_offset = cursor + cbfs_file_header_total_size("cbfs_master_header");

    if cursor + header_entry.len() > cbfs_end {
        panic!("Not enough space for CBFS header entry");
    }
    rom[cursor..cursor + header_entry.len()].copy_from_slice(&header_entry);
    cursor += header_entry.len();

    // Add user files
    for (name, path) in &files {
        let data = fs::read(path).unwrap_or_else(|e| panic!("Failed to read {:?}: {}", path, e));
        let entry = build_file_entry(name, CBFS_TYPE_RAW, &data);
        if cursor + entry.len() > cbfs_end {
            panic!(
                "Not enough space for file '{}' ({} bytes)",
                name,
                data.len()
            );
        }
        eprintln!("  {:40} offset={:#010x} size={}", name, cursor, data.len());
        rom[cursor..cursor + entry.len()].copy_from_slice(&entry);
        cursor += entry.len();
    }

    // Fill remaining space with a null entry
    let remaining = cbfs_end - cursor;
    if remaining >= cbfs_file_header_total_size("") {
        let null_entry = build_null_entry(remaining);
        rom[cursor..cursor + null_entry.len()].copy_from_slice(&null_entry);
    }

    // Write header pointer at last 4 bytes of ROM
    // This is the absolute 32-bit address where the header data lives in the memory map.
    // The ROM is mapped at (0x1_0000_0000 - size) .. 0xFFFFFFFF.
    let rom_base: u64 = 0x1_0000_0000u64 - size as u64;
    let header_addr = (rom_base + header_data_offset as u64) as u32;
    rom[size - 4..size].copy_from_slice(&header_addr.to_le_bytes());

    fs::write(&output, &rom).unwrap_or_else(|e| panic!("Failed to write {:?}: {}", output, e));
    eprintln!("Created CBFS image {:?} ({} bytes)", output, size);
}

fn cmd_list(input: PathBuf) {
    let rom = fs::read(&input).unwrap_or_else(|e| panic!("Failed to read {:?}: {}", input, e));

    let size = rom.len();

    // Read header pointer from last 4 bytes
    let ptr_bytes: [u8; 4] = rom[size - 4..size].try_into().unwrap();
    let header_addr = u32::from_le_bytes(ptr_bytes);
    let rom_base = 0x1_0000_0000u64 - size as u64;
    let header_offset = (header_addr as u64 - rom_base) as usize;

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

    // Walk file entries
    let entries_start = header.offset.get() as usize;
    let bb_offset = size - header.bootblocksize.get() as usize;
    let mut cursor = entries_start;

    println!(
        "{:40} {:>10} {:>10} {:>10}",
        "Name", "Offset", "Size", "Type"
    );
    println!("{}", "-".repeat(74));

    while cursor + CBFS_FILE_HEADER_SIZE <= bb_offset {
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

    // Show bootblock
    println!(
        "{:40} {:#010x} {:>10} {:>10}",
        "(bootblock)",
        bb_offset,
        header.bootblocksize.get(),
        "bootblock"
    );
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

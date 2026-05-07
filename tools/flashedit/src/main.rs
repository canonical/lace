// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! flashedit: Build and manipulate CBFS flash images

use clap::{Parser, Subcommand};
use lace_util::UsizeIsAtLeastU32;
use lace_util::cbfs::*;
use std::path::PathBuf;
use std::{fs, process};

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
        /// ROM size in bytes (e.g. 65536 for 64K, or use suffixes like 64K, 1M)
        #[arg(short, long, value_parser = parse_size)]
        size: u32,
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

/// Parses a ROM size, supporting suffixes K, M, G, ensuring minimum size of 4 bytes.
fn parse_size(s: &str) -> Result<u32, String> {
    let mut s = s.trim();
    let mut mul = 1u32;
    for (suf_str, suf_mul) in [("K", 1024), ("M", 1024 * 1024), ("G", 1024 * 1024 * 1024)] {
        if let Some(prefix) = s.strip_suffix(suf_str) {
            s = prefix.trim();
            mul = suf_mul;
            break;
        }
    }
    s.parse::<u32>()
        .ok()
        .and_then(|x| x.checked_mul(mul))
        .and_then(|x| if x >= 4 { Some(x) } else { None })
        .ok_or_else(|| format!("Invalid size {}", s))
}

fn parse_file_arg(s: &str) -> Result<(String, PathBuf), String> {
    let (name, path) = s.split_once('=').ok_or("expected name=path")?;
    if name.is_empty() {
        return Err("file name must not be empty".into());
    }
    if name.contains('\0') {
        return Err(format!(
            "file name '{}' contains NUL; CBFS names are NUL-terminated on flash",
            name.escape_debug()
        ));
    }
    Ok((name.to_string(), PathBuf::from(path)))
}

fn cmd_create(
    output: PathBuf,
    rom_size: u32,
    bootblock: PathBuf,
    files: Vec<(String, PathBuf)>,
) -> Result<(), String> {
    let bb_data = fs::read(&bootblock)
        .map_err(|e| format!("failed to read bootblock {}: {}", bootblock.display(), e))?;

    if bb_data.len() > rom_size.as_usize() {
        return Err(format!(
            "bootblock ({} bytes) exceeds ROM size ({} bytes)",
            bb_data.len(),
            rom_size
        ));
    }

    let rom_base: u32 = 0u32.wrapping_sub(rom_size);

    let mut rom = vec![0u8; rom_size.as_usize()];
    {
        let mut writer = CbfsWriter::create(&mut rom, rom_base, CBFS_DEFAULT_ALIGNMENT)
            .map_err(|e| format!("failed to create CBFS image: {e}"))?;

        // Add user files
        for (name, path) in &files {
            let data =
                fs::read(path).map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
            eprintln!("  {:40} size={}", name, data.len());
            writer
                .add_file(None, name.as_bytes(), CBFS_TYPE_RAW, &data)
                .map_err(|e| format!("failed to add file '{}': {e}", name))?;
        }

        // Add bootblock at top of ROM, leaving the last 4 bytes for
        // the CBFS header pointer which is written over the bootblock.
        if bb_data.len() < 4 {
            return Err("bootblock must be at least 4 bytes".into());
        }
        let bb_trimmed = &bb_data[..bb_data.len() - 4];
        let bb_content_offset =
            rom_size - u32::try_from(bb_data.len()).map_err(|_| "bootblock too large")?;
        eprintln!(
            "  {:40} offset={:#010x} size={}",
            "bootblock",
            bb_content_offset,
            bb_data.len()
        );
        writer
            .add_file(
                Some(bb_content_offset),
                b"bootblock",
                CBFS_TYPE_BOOTBLOCK,
                bb_trimmed,
            )
            .map_err(|e| format!("failed to add bootblock: {e}"))?;
    }

    fs::write(&output, &rom).map_err(|e| format!("failed to write {}: {}", output.display(), e))?;
    eprintln!(
        "Created CBFS image {} ({} bytes)",
        output.display(),
        rom_size
    );
    Ok(())
}

fn cmd_list(input: PathBuf) -> Result<(), String> {
    let rom = fs::read(&input).map_err(|e| format!("failed to read {}: {}", input.display(), e))?;

    let size = rom.len();
    let rom_size = u32::try_from(size).ok().filter(|_| size >= 4);
    let Some(rom_size) = rom_size else {
        return Err(format!(
            "ROM size ({}) is not a valid CBFS image size",
            size
        ));
    };

    let rom_base: u32 = 0u32.wrapping_sub(rom_size);
    let reader =
        CbfsReader::open(&rom, rom_base).map_err(|e| format!("failed to open CBFS image: {e}"))?;

    println!(
        "{:40} {:>10} {:>10} {:>10}",
        "Name", "Data Offset", "Size", "Type"
    );
    println!("{}", "-".repeat(74));

    for result in reader.files() {
        let file = result.map_err(|e| format!("corrupt CBFS entry: {e}"))?;
        let name = std::str::from_utf8(file.name).unwrap_or("???");
        let type_str = match file.r#type {
            CBFS_TYPE_NULL => "(empty)",
            CBFS_TYPE_BOOTBLOCK => "bootblock",
            CBFS_TYPE_RAW => "raw",
            CBFS_TYPE_DELETED => "deleted",
            _ => "unknown",
        };
        println!(
            "{:40} {:#010x} {:>10} {:>10}",
            name,
            file.offset,
            file.data.len(),
            type_str
        );
    }

    Ok(())
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Create {
            output,
            size,
            bootblock,
            files,
        } => cmd_create(output, size, bootblock, files),
        Command::List { input } => cmd_list(input),
    };
    if let Err(e) = result {
        eprintln!("flashedit: {}", e);
        process::exit(1);
    }
}

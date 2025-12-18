// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

use clap::Parser;
use collect_hwids::cli::Args;
use lace_util::chid::*;
use regex::bytes::Regex;
use std::error::Error;
use std::io::Write;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const SMBIOS_EP_PATH: &str = "/sys/firmware/dmi/tables/smbios_entry_point";
const SMBIOS_PATH: &str = "/sys/firmware/dmi/tables/DMI";

const DRM_PATH: &str = "/sys/class/drm";

fn main() {
    let args = Args::parse();

    // Read SMBIOS entry point and table
    let smbios_ep_data = match std::fs::read(SMBIOS_EP_PATH) {
        Ok(d) => Some(d),
        Err(e) => {
            eprintln!("warning: failed to read SMBIOS entry point: {}", e);
            None
        }
    };
    let smbios_data = match std::fs::read(SMBIOS_PATH) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: failed to read SMBIOS table: {}", e);
            std::process::exit(1);
        }
    };

    // Collect EDIDs
    let edids = collect_edids();
    let edid = if !edids.is_empty() {
        if edids.len() > 1 {
            eprintln!(
                "warning: more than one EDID found, using first one for internal panel ID, re-run with external screens disconnected"
            )
        }
        Some(edids[0].1.as_slice())
    } else {
        None
    };

    // Fill CHID sources
    let srcs =
        match chid_sources_from_smbios_and_edid(smbios_ep_data.as_deref(), &smbios_data, edid) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: failed to fill CHID sources: {}", e);
                std::process::exit(1);
            }
        };

    // Write sources and computed CHIDs to stdout
    let _ = write_sources_and_chids(&mut std::io::stdout(), &srcs);

    // Write output to ZIP archive
    if let Some(output) = &args.output {
        let outfile = match std::fs::File::create(output) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("error: failed to create {:?}: {}", args.output, e);
                std::process::exit(1);
            }
        };
        if let Err(e) = write_zip(
            outfile,
            smbios_ep_data.as_deref(),
            &smbios_data,
            &edids,
            &srcs,
        ) {
            eprintln!("error: failed to write zip {:?}: {}", &args.output, e);
            std::process::exit(1);
        }
    }
}

fn collect_edids() -> Vec<(String, Vec<u8>)> {
    let mut edids = Vec::new();

    let drm_dir = match std::fs::read_dir(DRM_PATH) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("warning: failed to open DRM directory: {}", e);
            return edids;
        }
    };
    let port_re = Regex::new(r"card\d+-").unwrap();

    for ent in drm_dir {
        let ent = match ent {
            Ok(e) => e,
            Err(e) => {
                eprintln!("warning: failed to read DRM directory: {}", e);
                continue;
            }
        };

        // Skip entries that don't look like a video port
        if !port_re.is_match(ent.path().as_os_str().as_encoded_bytes()) {
            continue;
        }

        // Try reading EDID
        let mut edid_path = ent.path();
        let port_name = edid_path.file_name().unwrap().to_string_lossy().to_string();
        edid_path.push("edid");

        match std::fs::read(&edid_path) {
            Ok(d) if d.is_empty() => (), // Skip empty EDID files
            Ok(d) => edids.push((port_name, d)),
            Err(e) => {
                eprintln!("warning: failed to read {:?}: {}", edid_path, e);
            }
        }
    }

    edids
}

fn write_sources_and_chids(
    w: &mut dyn std::io::Write,
    chid_srcs: &ChidSources,
) -> Result<(), Box<dyn Error>> {
    // Write CHID sources
    for (i, src) in chid_srcs.iter().enumerate() {
        if let Some(s) = src {
            writeln!(w, "CHID source {}: {:?}", i, s)?
        } else {
            writeln!(w, "CHID source {}: <missing>", i)?
        }
    }
    writeln!(w, "----------------------------------")?;
    // Write computed CHIDs
    for (i, &chid_type) in CHID_TYPES.iter().enumerate() {
        if let Some(chid) = compute_chid(chid_srcs, chid_type) {
            writeln!(w, "CHID type {}: {}", i, chid)?;
        } else {
            writeln!(w, "CHID type {}: <missing>", i)?;
        }
    }
    Ok(())
}

fn write_zip<W>(
    w: W,
    smbios_ep: Option<&[u8]>,
    smbios: &[u8],
    edids: &[(String, Vec<u8>)],
    chid_srcs: &ChidSources,
) -> Result<(), Box<dyn Error>>
where
    W: std::io::Write + std::io::Seek,
{
    let mut zw = ZipWriter::new(w);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    // Write SMBIOS entry point and table
    if let Some(smbios_ep) = smbios_ep {
        zw.start_file("smbios_entry_point.bin", options)?;
        let _ = zw.write(smbios_ep)?;
    }
    zw.start_file("smbios.bin", options)?;
    let _ = zw.write(smbios)?;
    // Write EDIDs
    for (port, edid) in edids.iter() {
        zw.start_file(format!("{}.bin", port), options)?;
        let _ = zw.write(edid)?;
    }
    // Text file for CHID sources and computed CHIDs
    zw.start_file("hwids.txt", options)?;
    // Write CHID sources and computed CHIDs
    write_sources_and_chids(&mut zw, chid_srcs)?;
    // Finish ZIP
    zw.finish()?;
    Ok(())
}

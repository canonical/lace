// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

use clap::Parser;
use lace_util::chid::*;
use lace_util::edid::*;
use lace_util::smbios::*;
use regex::bytes::Regex;
use std::error::Error;
use std::io::Write;
use std::{cmp::Ordering, ffi::OsString};
use zerocopy::FromBytes;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const SMBIOS_EP_PATH: &str = "/sys/firmware/dmi/tables/smbios_entry_point";
const SMBIOS_PATH: &str = "/sys/firmware/dmi/tables/DMI";

const DRM_PATH: &str = "/sys/class/drm";

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Where to write HWIDs zip archive
    #[arg(short, long)]
    output: Option<OsString>,
}

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

    // Fill CHID sources
    let srcs = fill_chid_sources(smbios_ep_data.as_deref(), &smbios_data, &edids);

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

fn fill_chid_sources(
    smbios_ep: Option<&[u8]>,
    smbios: &[u8],
    edids: &[(String, Vec<u8>)],
) -> ChidSources {
    fn str_from_maybe_u8(s: Option<&[u8]>) -> Option<String> {
        s.and_then(|s| str::from_utf8(s).ok()).map(|s| s.to_owned())
    }

    let mut chid_sources = ChidSources::default();

    if let Ok(type1) = find_smbios_table_by_type::<SmbiosTableType1>(smbios, 1) {
        chid_sources[CHID_SMBIOS_MANUFACTURER] =
            str_from_maybe_u8(type1.get_string(type1.table().manufacturer as usize));
        chid_sources[CHID_SMBIOS_FAMILY] =
            str_from_maybe_u8(type1.get_string(type1.table().family as usize));
        chid_sources[CHID_SMBIOS_PRODUCT_NAME] =
            str_from_maybe_u8(type1.get_string(type1.table().product_name as usize));
        chid_sources[CHID_SMBIOS_PRODUCT_SKU] =
            str_from_maybe_u8(type1.get_string(type1.table().sku_number as usize));
    }

    if let Ok(type2) = find_smbios_table_by_type::<SmbiosTableType2>(smbios, 2) {
        chid_sources[CHID_SMBIOS_BASEBOARD_MANUFACTURER] =
            str_from_maybe_u8(type2.get_string(type2.table().manufacturer as usize));
        chid_sources[CHID_SMBIOS_BASEBOARD_PRODUCT] =
            str_from_maybe_u8(type2.get_string(type2.table().product_name as usize));
    }

    let is_smbios_atleast_24 = smbios_ep
        .map(|ep| {
            let (maj, min) = if ep.starts_with(b"_SM3_") {
                let Ok((sm3_ep, _)) = Smbios3EntryPoint::ref_from_prefix(ep) else {
                    return false;
                };
                (sm3_ep.major_version, sm3_ep.minor_version)
            } else if ep.starts_with(b"_SM_") {
                let Ok((sm_ep, _)) = SmbiosEntryPoint::ref_from_prefix(ep) else {
                    return false;
                };
                (sm_ep.major_version, sm_ep.minor_version)
            } else {
                return false;
            };
            cmp_maj_min(maj, min, 2, 4).is_ge()
        })
        .unwrap_or_else(|| false);

    if is_smbios_atleast_24 {
        if let Ok(type0) = find_smbios_table_by_type::<SmbiosTableType0_24>(smbios, 0) {
            chid_sources[CHID_SMBIOS_BIOS_VENDOR] =
                str_from_maybe_u8(type0.get_string(type0.table().vendor as usize));
            chid_sources[CHID_SMBIOS_BIOS_VERSION] =
                str_from_maybe_u8(type0.get_string(type0.table().bios_version as usize));
            // These are defined to be in lower-case hex with 2-digit zero padding
            chid_sources[CHID_SMBIOS_BIOS_MAJOR] =
                Some(format!("{:02x}", type0.table().bios_major_release));
            chid_sources[CHID_SMBIOS_BIOS_MINOR] =
                Some(format!("{:02x}", type0.table().bios_minor_release));
        }
    } else if let Ok(type0) = find_smbios_table_by_type::<SmbiosTableType0>(smbios, 0) {
        chid_sources[CHID_SMBIOS_BIOS_VENDOR] =
            str_from_maybe_u8(type0.get_string(type0.table().vendor as usize));
        chid_sources[CHID_SMBIOS_BIOS_VERSION] =
            str_from_maybe_u8(type0.get_string(type0.table().bios_version as usize));
    }

    if let Ok(type3) = find_smbios_table_by_type::<SmbiosTableType3>(smbios, 3) {
        // This is defined to be in lower-case hex with no padding
        chid_sources[CHID_SMBIOS_ENCLOSURE_TYPE] = Some(format!("{:x}", type3.table().type_));
    }

    if !edids.is_empty() {
        if edids.len() > 1 {
            eprintln!(
                "warning: more than one EDID found, using first one for internal panel ID, re-run with external screens disconnected"
            )
        }
        match ParsedEdid::parse(&edids[0].1).and_then(|e| e.panel_id()) {
            Ok(panel_id) => chid_sources[CHID_EDID_PANEL] = Some(panel_id),
            Err(e) => {
                eprintln!("warning: failed to parse EDID for {}: {}", edids[0].0, e)
            }
        }
    }

    chid_sources
}

fn cmp_maj_min(maj_a: u8, min_a: u8, maj_b: u8, min_b: u8) -> Ordering {
    match maj_a.cmp(&maj_b) {
        Ordering::Equal => min_a.cmp(&min_b),
        ord => ord,
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

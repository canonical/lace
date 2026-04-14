// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! Computer Hardware ID (CHID) computation.
//!
//! CHIDs are GUIDs derived from system information (SMBIOS data, EDID panel
//! info, etc.) that uniquely identify a hardware configuration. They have two
//! primary uses:
//!
//! 1. **Firmware updates**: fwupd and the Linux Vendor Firmware Service (LVFS)
//!    use CHIDs to match firmware updates to specific devices.
//!
//! 2. **Device tree selection**: On machines that boot Linux with device tree,
//!    CHIDs are used to select the correct compatible string, which in turn
//!    determines which device tree to use from a list of available options.
//!
//! The algorithm follows Microsoft's CHID specification and RFC 9562 for UUID
//! generation:
//! 1. Concatenate selected hardware identifiers (separated by '&' in UCS-2
//!    encoding)
//! 2. Hash with SHA-1 using a defined namespace GUID
//! 3. Format the result as a version 5 (SHA-1 based) UUID per RFC 9562
//!
//! Different "CHID types" use different combinations of hardware sources,
//! allowing matching at varying levels of specificity (from exact BIOS version
//! down to just the manufacturer).

use crate::edid::*;
use crate::sha1::*;
use crate::smbios::*;
use crate::*;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem::size_of;
use lace_util_derive::Display;
use zerocopy::{FromBytes, IntoBytes};

/// CHID namespace GUID used as the seed for SHA-1 hashing per RFC 9562
pub const CHID_NAMESPACE_GUID: crate::Guid =
    crate::guid_str("70ffd812-4c7f-4c7d-0000-000000000000");

/// Indices into the ChidSources array for each possible hardware information
/// source. These correspond to SMBIOS table fields and EDID data used to
/// compute CHIDs.
///
/// From SMBIOS Type 1 (System Information):
pub const CHID_SMBIOS_MANUFACTURER: usize = 0;
pub const CHID_SMBIOS_FAMILY: usize = 1;
pub const CHID_SMBIOS_PRODUCT_NAME: usize = 2;
pub const CHID_SMBIOS_PRODUCT_SKU: usize = 3;
/// From SMBIOS Type 2 (Baseboard Information):
pub const CHID_SMBIOS_BASEBOARD_MANUFACTURER: usize = 4;
pub const CHID_SMBIOS_BASEBOARD_PRODUCT: usize = 5;
/// From SMBIOS Type 0 (BIOS Information):
pub const CHID_SMBIOS_BIOS_VENDOR: usize = 6;
pub const CHID_SMBIOS_BIOS_VERSION: usize = 7;
pub const CHID_SMBIOS_BIOS_MAJOR: usize = 8;
pub const CHID_SMBIOS_BIOS_MINOR: usize = 9;
/// From SMBIOS Type 3 (System Enclosure):
pub const CHID_SMBIOS_ENCLOSURE_TYPE: usize = 10;
/// From EDID (display panel identifier):
pub const CHID_EDID_PANEL: usize = 11;
/// Total number of possible CHID sources
pub const CHID_SOURCE_MAX: usize = 12;

/// CHID types and the sources of information they are generated from.
///
/// Each entry is a bitmask indicating which hardware sources are combined to
/// produce that CHID type. Types 0-14 are standard Microsoft CHID types,
/// ordered from most specific (type 0: exact BIOS version match) to least
/// specific (type 14: manufacturer only). Types 15-17 are non-standard
/// extensions that include EDID panel information for display-specific
/// firmware matching.
///
/// Firmware vendors typically publish updates targeting multiple CHID types,
/// allowing updates to match devices at the appropriate specificity level.
pub const CHID_TYPES: [usize; 18] = [
    // Type 0: Most specific - includes full BIOS version info
    // Manufacturer + Family + ProductName + SKU + BiosVendor + BiosVersion +
    // BiosMajor + BiosMinor
    1 << CHID_SMBIOS_MANUFACTURER
        | 1 << CHID_SMBIOS_FAMILY
        | 1 << CHID_SMBIOS_PRODUCT_NAME
        | 1 << CHID_SMBIOS_PRODUCT_SKU
        | 1 << CHID_SMBIOS_BIOS_VENDOR
        | 1 << CHID_SMBIOS_BIOS_VERSION
        | 1 << CHID_SMBIOS_BIOS_MAJOR
        | 1 << CHID_SMBIOS_BIOS_MINOR,
    // Type 1: Like type 0 but without SKU
    1 << CHID_SMBIOS_MANUFACTURER
        | 1 << CHID_SMBIOS_FAMILY
        | 1 << CHID_SMBIOS_PRODUCT_NAME
        | 1 << CHID_SMBIOS_BIOS_VENDOR
        | 1 << CHID_SMBIOS_BIOS_VERSION
        | 1 << CHID_SMBIOS_BIOS_MAJOR
        | 1 << CHID_SMBIOS_BIOS_MINOR,
    // Type 2: Like type 1 but without Family
    (1 << CHID_SMBIOS_MANUFACTURER)
        | (1 << CHID_SMBIOS_PRODUCT_NAME)
        | (1 << CHID_SMBIOS_BIOS_VENDOR)
        | (1 << CHID_SMBIOS_BIOS_VERSION)
        | (1 << CHID_SMBIOS_BIOS_MAJOR)
        | (1 << CHID_SMBIOS_BIOS_MINOR),
    // Type 3: System + baseboard info (no BIOS version)
    (1 << CHID_SMBIOS_MANUFACTURER)
        | (1 << CHID_SMBIOS_FAMILY)
        | (1 << CHID_SMBIOS_PRODUCT_NAME)
        | (1 << CHID_SMBIOS_PRODUCT_SKU)
        | (1 << CHID_SMBIOS_BASEBOARD_MANUFACTURER)
        | (1 << CHID_SMBIOS_BASEBOARD_PRODUCT),
    // Type 4: System info only (Manufacturer + Family + ProductName + SKU)
    (1 << CHID_SMBIOS_MANUFACTURER)
        | (1 << CHID_SMBIOS_FAMILY)
        | (1 << CHID_SMBIOS_PRODUCT_NAME)
        | (1 << CHID_SMBIOS_PRODUCT_SKU),
    // Type 5: Manufacturer + Family + ProductName
    (1 << CHID_SMBIOS_MANUFACTURER) | (1 << CHID_SMBIOS_FAMILY) | (1 << CHID_SMBIOS_PRODUCT_NAME),
    // Type 6: Manufacturer + SKU + baseboard info
    (1 << CHID_SMBIOS_MANUFACTURER)
        | (1 << CHID_SMBIOS_PRODUCT_SKU)
        | (1 << CHID_SMBIOS_BASEBOARD_MANUFACTURER)
        | (1 << CHID_SMBIOS_BASEBOARD_PRODUCT),
    // Type 7: Manufacturer + SKU only
    (1 << CHID_SMBIOS_MANUFACTURER) | (1 << CHID_SMBIOS_PRODUCT_SKU),
    // Type 8: Manufacturer + ProductName + baseboard info
    (1 << CHID_SMBIOS_MANUFACTURER)
        | (1 << CHID_SMBIOS_PRODUCT_NAME)
        | (1 << CHID_SMBIOS_BASEBOARD_MANUFACTURER)
        | (1 << CHID_SMBIOS_BASEBOARD_PRODUCT),
    // Type 9: Manufacturer + ProductName only
    (1 << CHID_SMBIOS_MANUFACTURER) | (1 << CHID_SMBIOS_PRODUCT_NAME),
    // Type 10: Manufacturer + Family + baseboard info
    (1 << CHID_SMBIOS_MANUFACTURER)
        | (1 << CHID_SMBIOS_FAMILY)
        | (1 << CHID_SMBIOS_BASEBOARD_MANUFACTURER)
        | (1 << CHID_SMBIOS_BASEBOARD_PRODUCT),
    // Type 11: Manufacturer + Family only
    (1 << CHID_SMBIOS_MANUFACTURER) | (1 << CHID_SMBIOS_FAMILY),
    // Type 12: Manufacturer + enclosure type (e.g., laptop vs desktop)
    (1 << CHID_SMBIOS_MANUFACTURER) | (1 << CHID_SMBIOS_ENCLOSURE_TYPE),
    // Type 13: Manufacturer + baseboard info only
    (1 << CHID_SMBIOS_MANUFACTURER)
        | (1 << CHID_SMBIOS_BASEBOARD_MANUFACTURER)
        | (1 << CHID_SMBIOS_BASEBOARD_PRODUCT),
    // Type 14: Manufacturer only - least specific standard type
    (1 << CHID_SMBIOS_MANUFACTURER),
    // Type 15 (non-standard): System info + EDID panel for display-specific fw
    (1 << CHID_SMBIOS_MANUFACTURER)
        | (1 << CHID_SMBIOS_FAMILY)
        | (1 << CHID_SMBIOS_PRODUCT_NAME)
        | (1 << CHID_EDID_PANEL),
    // Type 16 (non-standard): Manufacturer + Family + EDID panel
    (1 << CHID_SMBIOS_MANUFACTURER) | (1 << CHID_SMBIOS_FAMILY) | (1 << CHID_EDID_PANEL),
    // Type 17 (non-standard): Manufacturer + SKU + EDID panel
    (1 << CHID_SMBIOS_MANUFACTURER) | (1 << CHID_SMBIOS_PRODUCT_SKU) | (1 << CHID_EDID_PANEL),
];

/// Array of hardware source strings used to compute CHIDs. Each index
/// corresponds to a CHID_SMBIOS_* or CHID_EDID_* constant. `None` indicates
/// the source is not available on this system.
pub type ChidSources = [Option<String>; CHID_SOURCE_MAX];

#[derive(Clone, Copy, Debug, Display)]
pub enum ChidSourcesError {
    #[display("{}")]
    SmbiosError(SmbiosParseError),
    #[display("{}")]
    EdidError(EdidParseError),
}

impl From<SmbiosParseError> for ChidSourcesError {
    fn from(e: SmbiosParseError) -> Self {
        ChidSourcesError::SmbiosError(e)
    }
}

impl From<EdidParseError> for ChidSourcesError {
    fn from(e: EdidParseError) -> Self {
        ChidSourcesError::EdidError(e)
    }
}

pub fn chid_sources_from_smbios_and_edid(
    smbios_ep: Option<&[u8]>,
    smbios: &[u8],
    edid: Option<&[u8]>,
) -> Result<ChidSources, ChidSourcesError> {
    fn str_from_maybe_u8(s: Option<&[u8]>) -> Option<String> {
        s.and_then(|s| str::from_utf8(s).ok()).map(String::from)
    }

    let mut chid_sources = ChidSources::default();

    if let Some(type1) = find_smbios_table_by_type::<SmbiosTableType1>(smbios, 1)? {
        let table = type1.table();
        chid_sources[CHID_SMBIOS_MANUFACTURER] =
            str_from_maybe_u8(type1.get_string(table.manufacturer as usize));
        chid_sources[CHID_SMBIOS_FAMILY] =
            str_from_maybe_u8(type1.get_string(table.family as usize));
        chid_sources[CHID_SMBIOS_PRODUCT_NAME] =
            str_from_maybe_u8(type1.get_string(table.product_name as usize));
        chid_sources[CHID_SMBIOS_PRODUCT_SKU] =
            str_from_maybe_u8(type1.get_string(table.sku_number as usize));
    }

    if let Some(type2) = find_smbios_table_by_type::<SmbiosTableType2>(smbios, 2)? {
        let table = type2.table();
        chid_sources[CHID_SMBIOS_BASEBOARD_MANUFACTURER] =
            str_from_maybe_u8(type2.get_string(table.manufacturer as usize));
        chid_sources[CHID_SMBIOS_BASEBOARD_PRODUCT] =
            str_from_maybe_u8(type2.get_string(table.product_name as usize));
    }

    let is_smbios_atleast_24 = smbios_ep
        .map(|ep| {
            let (maj, min) = if ep.starts_with(b"_SM3_") {
                let Ok((sm3_ep, _)) = Smbios3EntryPoint::read_from_prefix(ep) else {
                    return false;
                };
                (sm3_ep.major_version, sm3_ep.minor_version)
            } else if ep.starts_with(b"_SM_") {
                let Ok((sm_ep, _)) = SmbiosEntryPoint::read_from_prefix(ep) else {
                    return false;
                };
                (sm_ep.major_version, sm_ep.minor_version)
            } else {
                return false;
            };
            (maj, min) >= (2, 4)
        })
        .unwrap_or_else(|| false);

    if is_smbios_atleast_24 {
        if let Some(type0) = find_smbios_table_by_type::<SmbiosTableType0_24>(smbios, 0)? {
            let table = type0.table();
            chid_sources[CHID_SMBIOS_BIOS_VENDOR] =
                str_from_maybe_u8(type0.get_string(table.vendor as usize));
            chid_sources[CHID_SMBIOS_BIOS_VERSION] =
                str_from_maybe_u8(type0.get_string(table.bios_version as usize));
            // These are defined to be in lower-case hex with 2-digit zero padding
            chid_sources[CHID_SMBIOS_BIOS_MAJOR] =
                Some(format!("{:02x}", table.bios_major_release));
            chid_sources[CHID_SMBIOS_BIOS_MINOR] =
                Some(format!("{:02x}", table.bios_minor_release));
        }
    } else if let Some(type0) = find_smbios_table_by_type::<SmbiosTableType0>(smbios, 0)? {
        let table = type0.table();
        chid_sources[CHID_SMBIOS_BIOS_VENDOR] =
            str_from_maybe_u8(type0.get_string(table.vendor as usize));
        chid_sources[CHID_SMBIOS_BIOS_VERSION] =
            str_from_maybe_u8(type0.get_string(table.bios_version as usize));
    }

    if let Some(type3) = find_smbios_table_by_type::<SmbiosTableType3>(smbios, 3)? {
        // This is defined to be in lower-case hex with no padding
        chid_sources[CHID_SMBIOS_ENCLOSURE_TYPE] = Some(format!("{:x}", type3.table().type_));
    }

    if let Some(edid) = edid {
        let parsed_edid = ParsedEdid::parse(edid)?;
        chid_sources[CHID_EDID_PANEL] = Some(parsed_edid.panel_id()?);
    }

    Ok(chid_sources)
}

/// Computes a CHID GUID from hardware source strings and a CHID type bitmask.
/// The `chid_type` selects which sources to include (use values from
/// [`CHID_TYPES`]). Returns `None` if any required source is missing.
///
/// The computation hashes the namespace GUID followed by the selected sources
/// (as UCS-2 strings separated by '&'), then formats the first 16 bytes of the
/// SHA-1 digest as a version 5 UUID per RFC 9562.
///
/// # Examples
///
/// ```
/// use lace_util::chid::*;
///
/// // Type 14 only requires manufacturer
/// let mut srcs: ChidSources = Default::default();
/// srcs[CHID_SMBIOS_MANUFACTURER] = Some("LENOVO".into());
///
/// let chid = compute_chid(&srcs, CHID_TYPES[14]).unwrap();
/// assert_eq!(chid, lace_util::guid_str("6de5d951-d755-576b-bd09-c5cf66b27234"));
/// ```
pub fn compute_chid(srcs: &ChidSources, chid_type: usize) -> Option<crate::Guid> {
    let mut ctx = Sha1Ctx::new();

    // Hash CHID namespace GUID in big-endian format (RFC 9562 requires big-
    // endian for the multi-byte fields when hashing)
    let mut ns_be = CHID_NAMESPACE_GUID;
    ns_be.data1 = ns_be.data1.to_be();
    ns_be.data2 = ns_be.data2.to_be();
    ns_be.data3 = ns_be.data3.to_be();
    ctx.update(ns_be.as_bytes());

    // Hash all selected sources as UCS-2 (UTF-16) strings, separated by '&'
    let mut first = true;
    for (i, src) in srcs.iter().enumerate() {
        if (chid_type & (1 << i)) != 0 {
            if !first {
                // UCS-2 encoded '&' separator between fields
                ctx.update(&[b'&', 0]);
            } else {
                first = false;
            }
            if let Some(src) = src {
                // Convert source string to UCS-2 (UTF-16) for hashing
                let ucs2: Vec<u16> = src.encode_utf16().collect();
                ctx.update(ucs2.as_bytes());
            } else {
                return None;
            }
        }
    }

    // Extract the GUID from first 16 bytes of SHA-1 digest (big-endian format)
    let digest = ctx.digest();
    let (mut chid, _) = unsafe {
        // SAFETY: SHA1 digest is 20 bytes, GUID is 16 bytes
        debug_assert!(SHA1_DIGEST_SIZE >= size_of::<crate::Guid>());
        crate::Guid::read_from_prefix(&digest).unwrap_unchecked()
    };

    // Convert from big-endian (hash output) to native-endian for GUID
    chid.data1 = u32::from_be(chid.data1);
    chid.data2 = u16::from_be(chid.data2);
    chid.data3 = u16::from_be(chid.data3);

    // Apply RFC 9562 version 5 (SHA-1 name-based) UUID formatting:
    // - Set version field (bits 12-15 of data3) to 5
    // - Set variant field (the two most significant bits (7-6) of data4[0]) to
    // binary 0b10 (i.e., pattern 10xxxxxx) per RFC 9562
    chid.data3 = (chid.data3 & 0x0fff) | (5 << 12);
    chid.data4[0] = (chid.data4[0] & 0x3f) | 0x80;

    Some(chid)
}

#[cfg(test)]
mod test {
    use super::*;

    fn assert_eq_all_chids(srcs: &ChidSources, expected_chids: &[Option<crate::Guid>]) {
        for (i, &chid_type) in CHID_TYPES.iter().enumerate() {
            if let Some(chid) = compute_chid(srcs, chid_type) {
                println!("CHID type {}: {}", i, chid);
            } else {
                println!("CHID type {}: <missing>", i);
            }
        }
        for (i, &chid_type) in CHID_TYPES.iter().enumerate() {
            let computed_chid = compute_chid(srcs, chid_type);
            assert_eq!(computed_chid, expected_chids[i], "CHID type {} mismatch", i);
        }
    }

    // These test cases are based on real data from real devices,
    // and corresponding known-good CHIDs generated by fwupd or systemd-analyze.

    #[test]
    fn test_lenovo_miix_630() {
        let srcs: ChidSources = [
            Some("LENOVO".into()),                             // CHID_SMBIOS_MANUFACTURER
            Some("Miix 630".into()),                           // CHID_SMBIOS_FAMILY
            Some("81F1".into()),                               // CHID_SMBIOS_PRODUCT_NAME
            Some("LENOVO_MT_81F1_BU_idea_FM_Miix 630".into()), // CHID_SMBIOS_PRODUCT_SKU
            Some("LENOVO".into()),                             // CHID_SMBIOS_BASEBOARD_MANUFACTURER
            Some("LNVNB161216".into()),                        // CHID_SMBIOS_BASEBOARD_PRODUCT
            Some("LENOVO".into()),                             // CHID_SMBIOS_BIOS_VENDOR
            Some("8WCN25WW".into()),                           // CHID_SMBIOS_BIOS_VERSION
            None,                                              // CHID_SMBIOS_BIOS_MAJOR
            None,                                              // CHID_SMBIOS_BIOS_MINOR
            Some("32".into()),                                 // CHID_SMBIOS_ENCLOSURE_TYPE
            None,                                              // CHID_EDID_PANEL
        ];

        let expected_chids = [
            None,
            None,
            None,
            Some(crate::guid_str("16a55446-eba9-5f97-80e3-5e39d8209bc3")),
            Some(crate::guid_str("c4c9a6be-5383-5de7-af35-c2de505edec8")),
            Some(crate::guid_str("14f581d2-d059-5cb2-9f8b-56d8be7932c9")),
            Some(crate::guid_str("a51054fb-5eef-594a-a5a0-cd87632d0aea")),
            Some(crate::guid_str("307ab358-ed84-57fe-bf05-e9195a28198d")),
            Some(crate::guid_str("7e613574-5445-5797-9567-2d0ed86e6ffa")),
            Some(crate::guid_str("b0f4463c-f851-5ec3-b031-2ccb873a609a")),
            Some(crate::guid_str("08b75d1f-6643-52a1-9bdd-071052860b33")),
            Some(crate::guid_str("dacf4a59-8e87-55c5-8b93-6912ded6bf7f")),
            Some(crate::guid_str("d0a8deb1-4cb5-50cd-bdda-595cfc13230c")),
            Some(crate::guid_str("71d86d4d-02f8-5566-a7a1-529cef184b7e")),
            Some(crate::guid_str("6de5d951-d755-576b-bd09-c5cf66b27234")),
            None,
            None,
            None,
        ];

        assert_eq_all_chids(&srcs, &expected_chids);
    }

    #[test]
    fn test_acer_aspire_a114_61() {
        let srcs: ChidSources = [
            Some("Acer".into()),
            Some("Aspire 1".into()),
            Some("Aspire A114-61".into()),
            Some("".into()),
            Some("S7C".into()),
            Some("Daisy_7C".into()),
            Some("Phoenix".into()),
            Some("V1.13".into()),
            Some("01".into()),
            Some("0d".into()),
            Some("a".into()),
            None,
        ];

        let expected_chids = [
            Some(crate::guid_str("45d37dbe-40fb-57bd-a257-55f422d4dc0a")),
            Some(crate::guid_str("373bfde5-ffaa-504c-84f3-f8f5357dfc29")),
            Some(crate::guid_str("e12521bf-0ed8-5406-af87-adad812c57c5")),
            Some(crate::guid_str("faa12ed4-bd49-5471-8f74-75c2267c3b46")),
            Some(crate::guid_str("965e3681-de3b-5e39-bb62-7d4917d7e36f")),
            Some(crate::guid_str("82fe1869-361c-56b2-b853-631747e64aa7")),
            Some(crate::guid_str("7e15f49e-04b4-5d56-a567-e7a15ba2aca1")),
            Some(crate::guid_str("7c107a7f-2d77-51aa-aef8-8d777e26ffbc")),
            Some(crate::guid_str("68b38fff-aadc-512c-937b-99d9c13eb484")),
            Some(crate::guid_str("260192d4-06d4-5124-ab46-ba210f4c14d7")),
            Some(crate::guid_str("175f000b-3d05-5c01-aedd-817b1a141f93")),
            Some(crate::guid_str("24277a94-7064-500f-9854-5264f20cfa99")),
            Some(crate::guid_str("92dcc94d-48f7-5ee8-b9ec-a6393fb7a484")),
            Some(crate::guid_str("d234a917-df0b-5453-a3d9-f27c06307395")),
            Some(crate::guid_str("1e301734-5d49-5df4-9ed2-aa1c0a9dddda")),
            None,
            None,
            None,
        ];

        assert_eq_all_chids(&srcs, &expected_chids);
    }

    #[test]
    fn test_lenovo_yoga_5g_14q8cx05() {
        let srcs: ChidSources = [
            Some("LENOVO".into()),
            Some("Yoga 5G 14Q8CX05".into()),
            Some("81XE".into()),
            Some("LENOVO_MT_81XE_BU_idea_FM_Yoga 5G 14Q8CX05".into()),
            Some("LENOVO".into()),
            Some("LNVNB161216".into()),
            Some("LENOVO".into()),
            Some("EACN41WW(V1.13)".into()),
            Some("01".into()),
            Some("29".into()),
            Some("1f".into()),
            None,
        ];

        let expected_chids = [
            Some(crate::guid_str("ea646c11-3da1-5c8d-9346-8ff156746650")),
            Some(crate::guid_str("5100eeed-c5e2-5b74-9c24-a22ca0644826")),
            Some(crate::guid_str("ddb3bcda-db7b-579d-9dd9-bcc4f5b052b8")),
            Some(crate::guid_str("fb364c09-efc0-5d16-ac97-0a3e6235b16c")),
            Some(crate::guid_str("7e7007ac-603c-55ef-bb77-3548784b9578")),
            Some(crate::guid_str("566b9ae8-a7fd-5c44-94d6-bac3e4cf38a7")),
            Some(crate::guid_str("6f3bdfb7-f832-5c5f-9777-9e3db35e22a6")),
            Some(crate::guid_str("c4ea686c-c56c-5e8e-a91e-89056683d417")),
            Some(crate::guid_str("a1a13249-2689-5c6d-a43f-98af040284c4")),
            Some(crate::guid_str("01439aea-e75c-5fbb-8842-18dcd1a7b8b3")),
            Some(crate::guid_str("65ab9f32-bbc8-52d3-87f9-b618fda7c07e")),
            Some(crate::guid_str("41ba2569-88df-57d4-b5e3-350ff985434a")),
            Some(crate::guid_str("32b7e294-a252-5a72-b3c6-6197f08c64f1")),
            Some(crate::guid_str("71d86d4d-02f8-5566-a7a1-529cef184b7e")),
            Some(crate::guid_str("6de5d951-d755-576b-bd09-c5cf66b27234")),
            None,
            None,
            None,
        ];

        assert_eq_all_chids(&srcs, &expected_chids);
    }

    #[test]
    fn test_lenovo_thinkpad_t14s_gen6() {
        let srcs: ChidSources = [
            Some("LENOVO".into()),
            Some("ThinkPad T14s Gen 6".into()),
            Some("21N2ZC5QUS".into()),
            Some("LENOVO_MT_21N2_BU_Think_FM_ThinkPad T14s Gen 6".into()),
            Some("LENOVO".into()),
            Some("21N2ZC5QUS".into()),
            Some("LENOVO".into()),
            Some("N42ET92W (2.22 )".into()),
            Some("02".into()),
            Some("16".into()),
            Some("a".into()),
            Some("BOE0b66".into()),
        ];

        let expected_chids = [
            Some(crate::guid_str("36bf13ab-c1b4-5e16-a9c9-96a8d6fd03b0")),
            Some(crate::guid_str("c8a30ee5-0482-5676-b108-676449dcf8aa")),
            Some(crate::guid_str("8e0c26cf-cccb-5a1c-bdd3-c1179224d5f4")),
            Some(crate::guid_str("f6cd4a9f-9632-516e-b748-65952f7380c5")),
            Some(crate::guid_str("a5a4e3c1-5922-5ed6-b78e-9f0ea873a988")),
            Some(crate::guid_str("a20ae3ec-49a1-5cb5-acb8-5d31c77b105a")),
            Some(crate::guid_str("8cfd85bb-0d77-59df-8546-264239be475e")),
            Some(crate::guid_str("513976f8-3f51-5b42-9ae0-931ce23c5f38")),
            Some(crate::guid_str("86a0d770-3ca1-57fa-ac05-413481c00a24")),
            Some(crate::guid_str("5c20e964-d530-5dd7-9efd-4aed9e73c3cb")),
            Some(crate::guid_str("d93b21c0-5ed9-5955-911a-5b15f114d786")),
            Some(crate::guid_str("431ff9e9-cd92-51c1-8917-46b0a0ef147c")),
            Some(crate::guid_str("e093d715-70f7-51f4-b6c8-b4a7e31def85")),
            Some(crate::guid_str("62631c9b-2642-5d4f-b7e2-1b917809d08d")),
            Some(crate::guid_str("6de5d951-d755-576b-bd09-c5cf66b27234")),
            Some(crate::guid_str("a8852b56-45ea-5377-ba2d-1910a5c897bb")),
            Some(crate::guid_str("1538c7fb-26b6-5144-b16f-2500b5a0a503")),
            Some(crate::guid_str("1480f3ca-b01a-5d7c-bbc9-7a17d7b4b58d")),
        ];

        assert_eq_all_chids(&srcs, &expected_chids);
    }

    #[test]
    fn test_chid_sources_error_from_smbios() {
        let smbios_err = crate::smbios::SmbiosParseError;
        let chid_err: ChidSourcesError = smbios_err.into();
        assert!(matches!(
            chid_err,
            ChidSourcesError::SmbiosError(crate::smbios::SmbiosParseError)
        ));
    }

    #[test]
    fn test_chid_sources_error_from_edid() {
        let edid_err = crate::edid::EdidParseError::InvalidHeader;
        let chid_err: ChidSourcesError = edid_err.into();
        assert!(matches!(
            chid_err,
            ChidSourcesError::EdidError(crate::edid::EdidParseError::InvalidHeader)
        ));
    }

    #[test]
    fn test_chid_sources_from_smbios_invalid_data() {
        // Test with truncated/invalid SMBIOS data - empty data will fail to parse
        let empty_smbios: &[u8] = &[];
        let result = chid_sources_from_smbios_and_edid(None, empty_smbios, None);
        // Empty SMBIOS data causes a parse error
        assert!(result.is_err(), "Should fail to parse empty SMBIOS data");
    }

    #[test]
    fn test_chid_sources_from_smbios_invalid_edid() {
        // Test with invalid EDID data and minimally valid SMBIOS data so that
        // we exercise the EDID error path rather than failing SMBIOS parsing.
        //
        // SMBIOS type 127 (end-of-table), length 4, handle 0xffff, followed by
        // double-NUL string terminator.
        let minimal_smbios: &[u8] = &[127, 4, 0xff, 0xff, 0, 0];
        let invalid_edid: &[u8] = &[0u8; 10]; // Too small to be valid EDID
        let result = chid_sources_from_smbios_and_edid(None, minimal_smbios, Some(invalid_edid));
        // Should fail specifically with EdidError for truly invalid EDID that
        // cannot be parsed.
        assert!(
            matches!(result, Err(ChidSourcesError::EdidError(_))),
            "Should fail with EdidError for invalid EDID"
        );
    }

    /// Helper to build a single SMBIOS table as raw bytes with a string section.
    fn smbios_table_bytes<T: IntoBytes + Immutable>(table: &T, strings: &[&str]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(table.as_bytes());
        for s in strings {
            data.extend_from_slice(s.as_bytes());
            data.push(0);
        }
        if strings.is_empty() {
            data.push(0);
        }
        data.push(0); // double-null terminator
        data
    }

    /// Helper to build a type 127 end-of-table marker.
    fn smbios_end_marker() -> Vec<u8> {
        let header = SmbiosHeader {
            type_: 127,
            length: size_of::<SmbiosHeader>() as u8,
            handle: [0, 0],
        };
        smbios_table_bytes(&header, &[])
    }

    /// Helper to build a valid 128-byte EDID with panel_id "BOE0b66".
    fn build_test_edid() -> [u8; 128] {
        let mut edid = [0u8; 128];
        // Magic header
        edid[0..8].copy_from_slice(&[0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00]);
        // Manufacturer "BOE": B=2, O=15, E=5 → (2<<10)|(15<<5)|5 = 0x09e5
        edid[8] = 0x09;
        edid[9] = 0xe5;
        // Product code 0x0b66 (little-endian) → hex string "0b66"
        edid[10] = 0x66;
        edid[11] = 0x0b;
        edid
    }

    fn build_type0(strings: &[&str]) -> Vec<u8> {
        let type0 = SmbiosTableType0 {
            header: SmbiosHeader {
                type_: 0,
                length: size_of::<SmbiosTableType0>() as u8,
                handle: [0, 0],
            },
            vendor: 1,
            bios_version: 2,
            bios_segment: 0,
            bios_release_date: 3,
            bios_size: 0,
            bios_characteristics: 0,
        };
        smbios_table_bytes(&type0, strings)
    }

    fn build_type0_24(major: u8, minor: u8, strings: &[&str]) -> Vec<u8> {
        let type0_24 = SmbiosTableType0_24 {
            header: SmbiosHeader {
                type_: 0,
                length: size_of::<SmbiosTableType0_24>() as u8,
                handle: [0, 0],
            },
            vendor: 1,
            bios_version: 2,
            bios_segment: 0,
            bios_release_date: 3,
            bios_size: 0,
            bios_characteristics: 0,
            bios_characteristics_ext: [0, 0],
            bios_major_release: major,
            bios_minor_release: minor,
            ec_fw_major_release: 0,
            ec_fw_minor_release: 0,
            ext_bios_rom_size: 0,
        };
        smbios_table_bytes(&type0_24, strings)
    }

    fn build_type1(strings: &[&str]) -> Vec<u8> {
        let type1 = SmbiosTableType1 {
            header: SmbiosHeader {
                type_: 1,
                length: size_of::<SmbiosTableType1>() as u8,
                handle: [0, 0],
            },
            manufacturer: 1,
            product_name: 2,
            version: 3,
            serial_number: 4,
            uuid: crate::Guid::default(),
            wake_up_type: 1,
            sku_number: 5,
            family: 6,
        };
        smbios_table_bytes(&type1, strings)
    }

    fn build_type2(strings: &[&str]) -> Vec<u8> {
        let type2 = SmbiosTableType2 {
            header: SmbiosHeader {
                type_: 2,
                length: size_of::<SmbiosTableType2>() as u8,
                handle: [0, 0],
            },
            manufacturer: 1,
            product_name: 2,
            version: 3,
            serial_number: 4,
        };
        smbios_table_bytes(&type2, strings)
    }

    fn build_type3(enclosure_type: u8, strings: &[&str]) -> Vec<u8> {
        let type3 = SmbiosTableType3 {
            header: SmbiosHeader {
                type_: 3,
                length: size_of::<SmbiosTableType3>() as u8,
                handle: [0, 0],
            },
            manufacturer: 1,
            type_: enclosure_type,
            version: 2,
            serial_number: 3,
            asset_tag_number: 4,
        };
        smbios_table_bytes(&type3, strings)
    }

    fn build_sm_ep(major: u8, minor: u8) -> Vec<u8> {
        let ep = SmbiosEntryPoint {
            anchor_string: *b"_SM_",
            entry_point_structure_checksum: 0,
            entry_point_length: size_of::<SmbiosEntryPoint>() as u8,
            major_version: major,
            minor_version: minor,
            max_structure_size: 0,
            entry_point_revision: 0,
            formatted_area: [0; 5],
            intermediate_anchor_string: *b"_DMI_",
            intermediate_checksum: 0,
            table_length: 0,
            table_address: 0,
            number_of_smbios_structures: 0,
            smbios_bcd_revision: 0,
        };
        ep.as_bytes().to_vec()
    }

    fn build_sm3_ep(major: u8, minor: u8) -> Vec<u8> {
        let ep = Smbios3EntryPoint {
            anchor_string: *b"_SM3_",
            entry_point_structure_checksum: 0,
            entry_point_length: size_of::<Smbios3EntryPoint>() as u8,
            major_version: major,
            minor_version: minor,
            docrev: 0,
            entry_point_revision: 0,
            reserved: 0,
            table_maximum_size: 0,
            table_address: 0,
        };
        ep.as_bytes().to_vec()
    }

    #[test]
    fn test_chid_sources_all_tables_no_ep() {
        // No entry point → is_smbios_atleast_24 = false → SmbiosTableType0 path
        let mut smbios = Vec::new();
        smbios.extend(build_type0(&["BIOSVendor", "BIOSVer", "01/01/2025"]));
        smbios.extend(build_type1(&[
            "TestMfr",
            "TestProduct",
            "Ver1",
            "Serial1",
            "TestSKU",
            "TestFamily",
        ]));
        smbios.extend(build_type2(&["BBMfr", "BBProduct", "BBVer", "BBSerial"]));
        smbios.extend(build_type3(
            0x0a,
            &["EncMfr", "EncVer", "EncSerial", "EncAsset"],
        ));
        smbios.extend(smbios_end_marker());

        let result = chid_sources_from_smbios_and_edid(None, &smbios, None).unwrap();

        assert_eq!(result[CHID_SMBIOS_MANUFACTURER].as_deref(), Some("TestMfr"));
        assert_eq!(result[CHID_SMBIOS_FAMILY].as_deref(), Some("TestFamily"));
        assert_eq!(
            result[CHID_SMBIOS_PRODUCT_NAME].as_deref(),
            Some("TestProduct")
        );
        assert_eq!(result[CHID_SMBIOS_PRODUCT_SKU].as_deref(), Some("TestSKU"));
        assert_eq!(
            result[CHID_SMBIOS_BASEBOARD_MANUFACTURER].as_deref(),
            Some("BBMfr")
        );
        assert_eq!(
            result[CHID_SMBIOS_BASEBOARD_PRODUCT].as_deref(),
            Some("BBProduct")
        );
        assert_eq!(
            result[CHID_SMBIOS_BIOS_VENDOR].as_deref(),
            Some("BIOSVendor")
        );
        assert_eq!(result[CHID_SMBIOS_BIOS_VERSION].as_deref(), Some("BIOSVer"));
        // No EP → no BIOS major/minor
        assert_eq!(result[CHID_SMBIOS_BIOS_MAJOR], None);
        assert_eq!(result[CHID_SMBIOS_BIOS_MINOR], None);
        // Enclosure type 0x0a → "a"
        assert_eq!(result[CHID_SMBIOS_ENCLOSURE_TYPE].as_deref(), Some("a"));
        assert_eq!(result[CHID_EDID_PANEL], None);
    }

    #[test]
    fn test_chid_sources_with_sm24_ep() {
        // SM_ entry point with version 2.4 → uses SmbiosTableType0_24 path
        let ep = build_sm_ep(2, 4);

        let mut smbios = Vec::new();
        smbios.extend(build_type0_24(1, 13, &["BV24", "BVer24", "01/01/2025"]));
        smbios.extend(smbios_end_marker());

        let result = chid_sources_from_smbios_and_edid(Some(&ep), &smbios, None).unwrap();

        assert_eq!(result[CHID_SMBIOS_BIOS_VENDOR].as_deref(), Some("BV24"));
        assert_eq!(result[CHID_SMBIOS_BIOS_VERSION].as_deref(), Some("BVer24"));
        // BIOS major=1 → "01", minor=13 → "0d"
        assert_eq!(result[CHID_SMBIOS_BIOS_MAJOR].as_deref(), Some("01"));
        assert_eq!(result[CHID_SMBIOS_BIOS_MINOR].as_deref(), Some("0d"));
    }

    #[test]
    fn test_chid_sources_with_sm3_ep() {
        // SM3_ entry point with version 3.0 → is_smbios_atleast_24 = true
        let ep = build_sm3_ep(3, 0);

        let mut smbios = Vec::new();
        smbios.extend(build_type0_24(2, 22, &["BV3", "BVer3", "01/01/2025"]));
        smbios.extend(smbios_end_marker());

        let result = chid_sources_from_smbios_and_edid(Some(&ep), &smbios, None).unwrap();

        assert_eq!(result[CHID_SMBIOS_BIOS_VENDOR].as_deref(), Some("BV3"));
        assert_eq!(result[CHID_SMBIOS_BIOS_VERSION].as_deref(), Some("BVer3"));
        // BIOS major=2 → "02", minor=22 → "16"
        assert_eq!(result[CHID_SMBIOS_BIOS_MAJOR].as_deref(), Some("02"));
        assert_eq!(result[CHID_SMBIOS_BIOS_MINOR].as_deref(), Some("16"));
    }

    #[test]
    fn test_chid_sources_with_invalid_ep_anchor() {
        // Entry point with unknown anchor → is_smbios_atleast_24 = false
        let ep = b"INVALID_ENTRY_POINT_DATA_PADDED_TO_32_BYTES!!";

        let mut smbios = Vec::new();
        smbios.extend(build_type0(&["BVInv", "BVerInv", "01/01/2025"]));
        smbios.extend(smbios_end_marker());

        let result = chid_sources_from_smbios_and_edid(Some(ep.as_slice()), &smbios, None).unwrap();

        // Falls through to SmbiosTableType0 (non-2.4) path
        assert_eq!(result[CHID_SMBIOS_BIOS_VENDOR].as_deref(), Some("BVInv"));
        assert_eq!(result[CHID_SMBIOS_BIOS_VERSION].as_deref(), Some("BVerInv"));
        assert_eq!(result[CHID_SMBIOS_BIOS_MAJOR], None);
        assert_eq!(result[CHID_SMBIOS_BIOS_MINOR], None);
    }

    #[test]
    fn test_chid_sources_with_sm_ep_too_short() {
        // SM_ anchor but data too short for SmbiosEntryPoint → return false
        let ep = b"_SM_TOO";

        let mut smbios = Vec::new();
        smbios.extend(build_type0(&["BVShort", "BVerShort", "01/01/2025"]));
        smbios.extend(smbios_end_marker());

        let result = chid_sources_from_smbios_and_edid(Some(ep.as_slice()), &smbios, None).unwrap();

        // Falls back to non-2.4 path
        assert_eq!(result[CHID_SMBIOS_BIOS_VENDOR].as_deref(), Some("BVShort"));
        assert_eq!(result[CHID_SMBIOS_BIOS_MAJOR], None);
    }

    #[test]
    fn test_chid_sources_with_sm3_ep_too_short() {
        // SM3_ anchor but data too short for Smbios3EntryPoint → return false
        let ep = b"_SM3_SHORT";

        let mut smbios = Vec::new();
        smbios.extend(build_type0(&["BVSm3S", "BVSm3S", "01/01/2025"]));
        smbios.extend(smbios_end_marker());

        let result = chid_sources_from_smbios_and_edid(Some(ep.as_slice()), &smbios, None).unwrap();

        // Falls back to non-2.4 path
        assert_eq!(result[CHID_SMBIOS_BIOS_VENDOR].as_deref(), Some("BVSm3S"));
        assert_eq!(result[CHID_SMBIOS_BIOS_MAJOR], None);
    }

    #[test]
    fn test_chid_sources_with_sm_ep_pre24() {
        // SM_ entry point with version 2.3 (< 2.4) → is_smbios_atleast_24 = false
        let ep = build_sm_ep(2, 3);

        let mut smbios = Vec::new();
        smbios.extend(build_type0(&["BVPre24", "BVerPre24", "01/01/2025"]));
        smbios.extend(smbios_end_marker());

        let result = chid_sources_from_smbios_and_edid(Some(&ep), &smbios, None).unwrap();

        assert_eq!(result[CHID_SMBIOS_BIOS_VENDOR].as_deref(), Some("BVPre24"));
        assert_eq!(
            result[CHID_SMBIOS_BIOS_VERSION].as_deref(),
            Some("BVerPre24")
        );
        assert_eq!(result[CHID_SMBIOS_BIOS_MAJOR], None);
        assert_eq!(result[CHID_SMBIOS_BIOS_MINOR], None);
    }

    #[test]
    fn test_chid_sources_with_edid() {
        let mut smbios = Vec::new();
        smbios.extend(build_type1(&["Mfr", "Prod", "V", "S", "SKU", "Fam"]));
        smbios.extend(smbios_end_marker());

        let edid = build_test_edid();
        let result = chid_sources_from_smbios_and_edid(None, &smbios, Some(&edid)).unwrap();

        assert_eq!(result[CHID_SMBIOS_MANUFACTURER].as_deref(), Some("Mfr"));
        assert_eq!(result[CHID_EDID_PANEL].as_deref(), Some("BOE0b66"));
    }

    #[test]
    fn test_chid_sources_missing_optional_tables() {
        // SMBIOS with only type1 and end marker — type0, type2, type3 absent
        let mut smbios = Vec::new();
        smbios.extend(build_type1(&[
            "OnlyMfr", "OnlyProd", "V", "S", "OnlySKU", "OnlyFam",
        ]));
        smbios.extend(smbios_end_marker());

        let result = chid_sources_from_smbios_and_edid(None, &smbios, None).unwrap();

        assert_eq!(result[CHID_SMBIOS_MANUFACTURER].as_deref(), Some("OnlyMfr"));
        assert_eq!(result[CHID_SMBIOS_FAMILY].as_deref(), Some("OnlyFam"));
        assert_eq!(
            result[CHID_SMBIOS_PRODUCT_NAME].as_deref(),
            Some("OnlyProd")
        );
        assert_eq!(result[CHID_SMBIOS_PRODUCT_SKU].as_deref(), Some("OnlySKU"));
        // No type2 → baseboard fields are None
        assert_eq!(result[CHID_SMBIOS_BASEBOARD_MANUFACTURER], None);
        assert_eq!(result[CHID_SMBIOS_BASEBOARD_PRODUCT], None);
        // No type0 → BIOS fields are None
        assert_eq!(result[CHID_SMBIOS_BIOS_VENDOR], None);
        assert_eq!(result[CHID_SMBIOS_BIOS_VERSION], None);
        assert_eq!(result[CHID_SMBIOS_BIOS_MAJOR], None);
        assert_eq!(result[CHID_SMBIOS_BIOS_MINOR], None);
        // No type3 → enclosure is None
        assert_eq!(result[CHID_SMBIOS_ENCLOSURE_TYPE], None);
        assert_eq!(result[CHID_EDID_PANEL], None);
    }

    #[test]
    fn test_compute_chid_missing_required_source() {
        // Type 14 requires only manufacturer. If all sources are None,
        // compute_chid should return None.
        let srcs: ChidSources = Default::default();
        assert_eq!(compute_chid(&srcs, CHID_TYPES[14]), None);
    }

    #[test]
    fn test_chid_sources_all_tables_with_sm24_ep() {
        // Full end-to-end: all tables + SM_ 2.4 EP + EDID, then compute CHIDs
        let ep = build_sm_ep(2, 4);

        let mut smbios = Vec::new();
        smbios.extend(build_type0_24(
            1,
            0x0d,
            &["LENOVO", "N42ET92W (2.22 )", "01/01/2025"],
        ));
        smbios.extend(build_type1(&[
            "LENOVO",
            "ThinkPad T14s Gen 6",
            "Ver",
            "Serial",
            "LENOVO_MT_21N2_BU_Think_FM_ThinkPad T14s Gen 6",
            "ThinkPad T14s Gen 6",
        ]));
        smbios.extend(build_type2(&["LENOVO", "21N2ZC5QUS", "BBVer", "BBSerial"]));
        smbios.extend(build_type3(
            0x0a,
            &["EncMfr", "EncVer", "EncSerial", "EncAsset"],
        ));
        smbios.extend(smbios_end_marker());

        let edid = build_test_edid();
        let result = chid_sources_from_smbios_and_edid(Some(&ep), &smbios, Some(&edid)).unwrap();

        assert_eq!(result[CHID_SMBIOS_MANUFACTURER].as_deref(), Some("LENOVO"));
        assert_eq!(
            result[CHID_SMBIOS_FAMILY].as_deref(),
            Some("ThinkPad T14s Gen 6")
        );
        assert_eq!(result[CHID_SMBIOS_BIOS_VENDOR].as_deref(), Some("LENOVO"));
        assert_eq!(
            result[CHID_SMBIOS_BIOS_VERSION].as_deref(),
            Some("N42ET92W (2.22 )")
        );
        assert_eq!(result[CHID_SMBIOS_BIOS_MAJOR].as_deref(), Some("01"));
        assert_eq!(result[CHID_SMBIOS_BIOS_MINOR].as_deref(), Some("0d"));
        assert_eq!(
            result[CHID_SMBIOS_BASEBOARD_MANUFACTURER].as_deref(),
            Some("LENOVO")
        );
        assert_eq!(
            result[CHID_SMBIOS_BASEBOARD_PRODUCT].as_deref(),
            Some("21N2ZC5QUS")
        );
        assert_eq!(result[CHID_SMBIOS_ENCLOSURE_TYPE].as_deref(), Some("a"));
        assert_eq!(result[CHID_EDID_PANEL].as_deref(), Some("BOE0b66"));

        // Verify compute_chid works with parsed sources (type 14 = manufacturer only)
        let chid = compute_chid(&result, CHID_TYPES[14]).unwrap();
        assert_eq!(
            chid,
            crate::guid_str("6de5d951-d755-576b-bd09-c5cf66b27234")
        );
    }
}

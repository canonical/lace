// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

use crate::edid::*;
use crate::sha1::*;
use crate::smbios::*;
use crate::*;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem::size_of;
use zerocopy::{FromBytes, IntoBytes};

/// CHID namespace GUID
pub const CHID_NAMESPACE_GUID: crate::Guid =
    crate::guid_str("70ffd812-4c7f-4c7d-0000-000000000000");

/// Possible sources of information for a CHID type
pub const CHID_SMBIOS_MANUFACTURER: usize = 0;
pub const CHID_SMBIOS_FAMILY: usize = 1;
pub const CHID_SMBIOS_PRODUCT_NAME: usize = 2;
pub const CHID_SMBIOS_PRODUCT_SKU: usize = 3;
pub const CHID_SMBIOS_BASEBOARD_MANUFACTURER: usize = 4;
pub const CHID_SMBIOS_BASEBOARD_PRODUCT: usize = 5;
pub const CHID_SMBIOS_BIOS_VENDOR: usize = 6;
pub const CHID_SMBIOS_BIOS_VERSION: usize = 7;
pub const CHID_SMBIOS_BIOS_MAJOR: usize = 8;
pub const CHID_SMBIOS_BIOS_MINOR: usize = 9;
pub const CHID_SMBIOS_ENCLOSURE_TYPE: usize = 10;
pub const CHID_EDID_PANEL: usize = 11;
pub const CHID_SOURCE_MAX: usize = 12;

/// CHID types and the exact sources of information they are generated from
pub const CHID_TYPES: [usize; 18] = [
    // 0
    1 << CHID_SMBIOS_MANUFACTURER
        | 1 << CHID_SMBIOS_FAMILY
        | 1 << CHID_SMBIOS_PRODUCT_NAME
        | 1 << CHID_SMBIOS_PRODUCT_SKU
        | 1 << CHID_SMBIOS_BIOS_VENDOR
        | 1 << CHID_SMBIOS_BIOS_VERSION
        | 1 << CHID_SMBIOS_BIOS_MAJOR
        | 1 << CHID_SMBIOS_BIOS_MINOR,
    // 1
    1 << CHID_SMBIOS_MANUFACTURER
        | 1 << CHID_SMBIOS_FAMILY
        | 1 << CHID_SMBIOS_PRODUCT_NAME
        | 1 << CHID_SMBIOS_BIOS_VENDOR
        | 1 << CHID_SMBIOS_BIOS_VERSION
        | 1 << CHID_SMBIOS_BIOS_MAJOR
        | 1 << CHID_SMBIOS_BIOS_MINOR,
    // 2
    (1 << CHID_SMBIOS_MANUFACTURER)
        | (1 << CHID_SMBIOS_PRODUCT_NAME)
        | (1 << CHID_SMBIOS_BIOS_VENDOR)
        | (1 << CHID_SMBIOS_BIOS_VERSION)
        | (1 << CHID_SMBIOS_BIOS_MAJOR)
        | (1 << CHID_SMBIOS_BIOS_MINOR),
    // 3
    (1 << CHID_SMBIOS_MANUFACTURER)
        | (1 << CHID_SMBIOS_FAMILY)
        | (1 << CHID_SMBIOS_PRODUCT_NAME)
        | (1 << CHID_SMBIOS_PRODUCT_SKU)
        | (1 << CHID_SMBIOS_BASEBOARD_MANUFACTURER)
        | (1 << CHID_SMBIOS_BASEBOARD_PRODUCT),
    // 4
    (1 << CHID_SMBIOS_MANUFACTURER)
        | (1 << CHID_SMBIOS_FAMILY)
        | (1 << CHID_SMBIOS_PRODUCT_NAME)
        | (1 << CHID_SMBIOS_PRODUCT_SKU),
    // 5
    (1 << CHID_SMBIOS_MANUFACTURER) | (1 << CHID_SMBIOS_FAMILY) | (1 << CHID_SMBIOS_PRODUCT_NAME),
    // 6
    (1 << CHID_SMBIOS_MANUFACTURER)
        | (1 << CHID_SMBIOS_PRODUCT_SKU)
        | (1 << CHID_SMBIOS_BASEBOARD_MANUFACTURER)
        | (1 << CHID_SMBIOS_BASEBOARD_PRODUCT),
    // 7
    (1 << CHID_SMBIOS_MANUFACTURER) | (1 << CHID_SMBIOS_PRODUCT_SKU),
    // 8
    (1 << CHID_SMBIOS_MANUFACTURER)
        | (1 << CHID_SMBIOS_PRODUCT_NAME)
        | (1 << CHID_SMBIOS_BASEBOARD_MANUFACTURER)
        | (1 << CHID_SMBIOS_BASEBOARD_PRODUCT),
    // 9
    (1 << CHID_SMBIOS_MANUFACTURER) | (1 << CHID_SMBIOS_PRODUCT_NAME),
    // 10
    (1 << CHID_SMBIOS_MANUFACTURER)
        | (1 << CHID_SMBIOS_FAMILY)
        | (1 << CHID_SMBIOS_BASEBOARD_MANUFACTURER)
        | (1 << CHID_SMBIOS_BASEBOARD_PRODUCT),
    // 11
    (1 << CHID_SMBIOS_MANUFACTURER) | (1 << CHID_SMBIOS_FAMILY),
    // 12
    (1 << CHID_SMBIOS_MANUFACTURER) | (1 << CHID_SMBIOS_ENCLOSURE_TYPE),
    // 13
    (1 << CHID_SMBIOS_MANUFACTURER)
        | (1 << CHID_SMBIOS_BASEBOARD_MANUFACTURER)
        | (1 << CHID_SMBIOS_BASEBOARD_PRODUCT),
    // 14
    (1 << CHID_SMBIOS_MANUFACTURER),
    // 15 (non-standard)
    (1 << CHID_SMBIOS_MANUFACTURER)
        | (1 << CHID_SMBIOS_FAMILY)
        | (1 << CHID_SMBIOS_PRODUCT_NAME)
        | (1 << CHID_EDID_PANEL),
    // 16 (non-standard)
    (1 << CHID_SMBIOS_MANUFACTURER) | (1 << CHID_SMBIOS_FAMILY) | (1 << CHID_EDID_PANEL),
    // 17 (non-standard)
    (1 << CHID_SMBIOS_MANUFACTURER) | (1 << CHID_SMBIOS_PRODUCT_SKU) | (1 << CHID_EDID_PANEL),
];

pub type ChidSources = [Option<String>; CHID_SOURCE_MAX];

#[derive(Clone, Copy, Debug)]
pub enum ChidSourcesError {
    SmbiosError(SmbiosParseError),
    EdidError(EdidParseError),
}

impl Display for ChidSourcesError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ChidSourcesError::SmbiosError(e) => Display::fmt(e, f),
            ChidSourcesError::EdidError(e) => Display::fmt(e, f),
        }
    }
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
            cmp_maj_min(maj, min, 2, 4).is_ge()
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

pub fn compute_chid(srcs: &ChidSources, chid_type: usize) -> Option<crate::Guid> {
    let mut ctx = Sha1Ctx::new();

    // Hash CHID namespace GUID in big-endian format
    let mut ns_be = CHID_NAMESPACE_GUID;
    ns_be.data1 = ns_be.data1.to_be();
    ns_be.data2 = ns_be.data2.to_be();
    ns_be.data3 = ns_be.data3.to_be();
    ctx.update(ns_be.as_bytes());

    // Hash all selected sources
    let mut first = true;
    for (i, src) in srcs.iter().enumerate() {
        if (chid_type & (1 << i)) != 0 {
            if !first {
                // Separator
                ctx.update(&[b'&', 0]);
            } else {
                first = false;
            }
            if let Some(src) = src {
                let ucs2: Vec<u16> = src.encode_utf16().collect();
                ctx.update(ucs2.as_bytes());
            } else {
                // Missing source means this chid is missing
                return None;
            }
        }
    }

    // First 16-bytes of the digests are the CHID in big-endian format
    let digest = ctx.digest();
    let (mut chid, _) = unsafe {
        // SAFETY: SHA1 digest is 20 bytes, GUID is 16 bytes
        debug_assert!(SHA1_DIGEST_SIZE >= size_of::<crate::Guid>());
        crate::Guid::read_from_prefix(&digest).unwrap_unchecked()
    };
    // Convert to native-endian format
    chid.data1 = u32::from_be(chid.data1);
    chid.data2 = u16::from_be(chid.data2);
    chid.data3 = u16::from_be(chid.data3);
    // Fix-up fields to conform to RFC 4122
    chid.data3 = (chid.data3 & 0x0FFF) | (5 << 12);
    chid.data4[0] = (chid.data4[0] & 0x3F) | 0x80;
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
}

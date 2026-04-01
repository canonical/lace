// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! CHID mapping parser
//! This is same format used in systemd UEFI ".hwids" section.
//! It is a list of length prefixed entries, terminated by an entry with zero type and length.
//! The entries are followed by a table of NUL-terminated strings, which are referenced by the
//! entries by absolute offset.

use crate::{Display, Guid};
use core::mem::size_of;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

// (Private) definitions for the binary serialization format
#[derive(Default, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
struct ChidMappingHeader {
    descriptor: u32, // Upper 4 bits: type, lower 28 bits: length
    chid: Guid,
}

const CHID_MAPPING_DESCRIPTOR_DEVICE_TREE: u32 = 0x1;
const CHID_MAPPING_DESCRIPTOR_UEFI_FW: u32 = 0x2;

#[derive(FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
struct ChidMappingDeviceTree {
    name_offset: u32,
    compatible_offset: u32,
}

#[derive(FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
struct ChidMappingUefiFw {
    name_offset: u32,
    fwid_offset: u32,
}

/// Parsed CHID mapping entry
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChidMapping<'s> {
    DeviceTree {
        chid: Guid,
        name: Option<&'s str>,
        compatible: Option<&'s str>,
    },
    UefiFw {
        chid: Guid,
        name: Option<&'s str>,
        fwid: Option<&'s str>,
    },
    Unknown {
        kind: u32,
        chid: Guid,
        body: &'s [u8],
    },
}

/// Error indicating malformed CHID mappings data
#[derive(Clone, Copy, Default, Debug, Display)]
#[display("malformed CHID mappings data")]
pub struct MalformedChidMappings;

pub struct ChidMappingIterator<'s> {
    data: &'s [u8],
    remaining: &'s [u8],
}

impl<'s> From<&'s [u8]> for ChidMappingIterator<'s> {
    /// Create an iterator over CHID mappings from the given data slice
    fn from(data: &'s [u8]) -> Self {
        Self {
            data,
            remaining: data,
        }
    }
}

impl<'s> Iterator for ChidMappingIterator<'s> {
    type Item = Result<ChidMapping<'s>, MalformedChidMappings>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_chid_mapping().transpose()
    }
}

impl<'s> ChidMappingIterator<'s> {
    /// Parse the next CHID mapping entry from remaining data
    fn next_chid_mapping(&mut self) -> Result<Option<ChidMapping<'s>>, MalformedChidMappings> {
        let (header, body) = ChidMappingHeader::read_from_prefix(self.remaining)
            .map_err(|_| MalformedChidMappings)?; // Terminator always needs to be present
        if header.descriptor == 0 {
            // End marker entry
            return Ok(None);
        }
        let entry_length: usize = (header.descriptor & 0x0fff_ffff) as usize;
        let body = entry_length
            .checked_sub(size_of::<ChidMappingHeader>())
            .and_then(|body_len| body.get(..body_len))
            .ok_or(MalformedChidMappings)?; // Entry truncated
        let mapping = match header.descriptor >> 28 {
            CHID_MAPPING_DESCRIPTOR_DEVICE_TREE => {
                let (body, _) = ChidMappingDeviceTree::read_from_prefix(body)
                    .map_err(|_| MalformedChidMappings)?; // Entry truncated
                ChidMapping::DeviceTree {
                    chid: header.chid,
                    name: self.extract_string(body.name_offset as usize)?,
                    compatible: self.extract_string(body.compatible_offset as usize)?,
                }
            }
            CHID_MAPPING_DESCRIPTOR_UEFI_FW => {
                let (body, _) =
                    ChidMappingUefiFw::read_from_prefix(body).map_err(|_| MalformedChidMappings)?; // Entry truncated
                ChidMapping::UefiFw {
                    chid: header.chid,
                    name: self.extract_string(body.name_offset as usize)?,
                    fwid: self.extract_string(body.fwid_offset as usize)?,
                }
            }
            _ => ChidMapping::Unknown {
                kind: header.descriptor >> 28,
                chid: header.chid,
                body,
            },
        };
        self.remaining = &self.remaining[entry_length..]; // Advance to next entry
        Ok(Some(mapping))
    }

    /// Extract a NUL-terminated string from data at the given offset (or None if offset is 0)
    fn extract_string(&self, offset: usize) -> Result<Option<&'s str>, MalformedChidMappings> {
        if offset == 0 {
            return Ok(None);
        }
        let slice = self.data.get(offset..).ok_or(MalformedChidMappings)?;
        let nul_pos = slice
            .iter()
            .position(|&b| b == 0)
            .ok_or(MalformedChidMappings)?;
        str::from_utf8(&slice[..nul_pos])
            .map_err(|_| MalformedChidMappings)
            .map(Some)
    }
}

impl ChidMapping<'_> {
    /// Returns the CHID of the mapping
    pub fn chid(&self) -> &Guid {
        match self {
            ChidMapping::DeviceTree { chid, .. } => chid,
            ChidMapping::UefiFw { chid, .. } => chid,
            ChidMapping::Unknown { chid, .. } => chid,
        }
    }

    /// Provides the kind/type of the mapping when serialized
    #[cfg(feature = "std")]
    fn serialized_kind(&self) -> u32 {
        match self {
            ChidMapping::DeviceTree { .. } => CHID_MAPPING_DESCRIPTOR_DEVICE_TREE,
            ChidMapping::UefiFw { .. } => CHID_MAPPING_DESCRIPTOR_UEFI_FW,
            ChidMapping::Unknown { kind, .. } => *kind,
        }
    }

    /// Calculates the size of the mapping when serialized
    #[cfg(feature = "std")]
    fn serialized_size(&self) -> usize {
        size_of::<ChidMappingHeader>()
            + match self {
                ChidMapping::DeviceTree { .. } => size_of::<ChidMappingDeviceTree>(),
                ChidMapping::UefiFw { .. } => size_of::<ChidMappingUefiFw>(),
                ChidMapping::Unknown { body, .. } => body.len(),
            }
    }
}

/// Serialize a set of CHID mappings to the given writer
/// Serializing Unknown mappings is supported, but their body is written as-is
/// which means that any string offsets inside them will likely be invalid.
#[cfg(feature = "std")]
pub fn serialize_chid_mappings<'s, W: std::io::Write>(
    mut w: W,
    mappings: &[ChidMapping<'s>],
) -> std::io::Result<()> {
    // Figure out the size of formatted data to know where string data starts
    let formatted_size: usize = mappings.iter().map(ChidMapping::serialized_size).sum();
    let formatted_size = formatted_size + size_of::<ChidMappingHeader>() + 8; // Terminator entry + its body
    // Buffer to store the strings until we write them
    let mut string_data: Vec<u8> = Vec::new();
    // De-duplication map for strings
    let mut string_dedup = std::collections::HashMap::<&'s str, u32>::new();

    // Write a serialized string to string_data and return its offset
    let mut serialize_string = |s: Option<&'s str>| {
        if let Some(s) = s {
            if let Some(&offset) = string_dedup.get(s) {
                return offset;
            }
            let offset = formatted_size + string_data.len();
            string_data.extend_from_slice(s.as_bytes());
            string_data.push(0); // NUL terminator
            string_dedup.insert(s, offset as u32);
            offset as u32
        } else {
            0
        }
    };

    for mapping in mappings.iter() {
        w.write_all(
            ChidMappingHeader {
                descriptor: (mapping.serialized_kind() << 28) | mapping.serialized_size() as u32,
                chid: *mapping.chid(),
            }
            .as_bytes(),
        )?;

        match mapping {
            ChidMapping::DeviceTree {
                name, compatible, ..
            } => {
                w.write_all(
                    ChidMappingDeviceTree {
                        name_offset: serialize_string(*name),
                        compatible_offset: serialize_string(*compatible),
                    }
                    .as_bytes(),
                )?;
            }
            ChidMapping::UefiFw { name, fwid, .. } => {
                w.write_all(
                    ChidMappingUefiFw {
                        name_offset: serialize_string(*name),
                        fwid_offset: serialize_string(*fwid),
                    }
                    .as_bytes(),
                )?;
            }
            ChidMapping::Unknown {
                kind: _,
                chid: _,
                body,
            } => {
                w.write_all(body)?;
            }
        }
    }

    // Write the terminator entry
    // NOTE: systemd-ukify writes a full sized entry with 2 x u32s as body here, so
    // we do the same to make the outputs compatible with possibly broken parsers.
    w.write_all(ChidMappingHeader::default().as_bytes())?;
    w.write_all(&[0u8; 8])?;

    // Write string data
    w.write_all(&string_data)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guid_str;

    fn push_mapping(data: &mut Vec<u8>, header: ChidMappingHeader, body: &[u8]) {
        data.extend_from_slice(header.as_bytes());
        data.extend_from_slice(body);
    }

    fn push_strings(data: &mut Vec<u8>, strings: &[&str]) {
        for s in strings.iter() {
            data.extend_from_slice(s.as_bytes());
            data.push(0); // NUL terminator
        }
    }

    fn parse_chid_mappings<'s>(
        data: &'s [u8],
    ) -> Result<Vec<ChidMapping<'s>>, MalformedChidMappings> {
        ChidMappingIterator::from(data).collect::<Result<Vec<_>, _>>()
    }

    // Error cases

    #[test]
    fn test_empty_no_terminator() {
        assert!(parse_chid_mappings(&[]).is_err());
    }

    #[test]
    fn test_entry_length_does_not_cover_header() {
        let mut data = Vec::new();
        push_mapping(
            &mut data,
            ChidMappingHeader {
                descriptor: 0x5000_0004,
                chid: Guid::default(),
            },
            &[],
        ); // type 5, length 4 (too small for header)
        push_mapping(&mut data, ChidMappingHeader::default(), &[]); // terminator
        assert!(parse_chid_mappings(&data).is_err());
    }

    #[test]
    fn test_entry_length_exceeds_data() {
        let mut data = Vec::new();
        push_mapping(
            &mut data,
            ChidMappingHeader {
                descriptor: 0x6000_0040,
                chid: Guid::default(),
            },
            &[],
        ); // type 6, length 64 (larger than data)
        push_mapping(&mut data, ChidMappingHeader::default(), &[]); // terminator
        assert!(parse_chid_mappings(&data).is_err());
    }

    #[test]
    fn test_entry_length_does_not_cover_body() {
        let mut data = Vec::new();
        push_mapping(
            &mut data,
            ChidMappingHeader {
                descriptor: 0x1000_0014,
                chid: Guid::default(),
            },
            ChidMappingDeviceTree {
                name_offset: 0,
                compatible_offset: 0,
            }
            .as_bytes(),
        ); // type 1, length 20 (too small for body)
        push_mapping(&mut data, ChidMappingHeader::default(), &[]); // terminator
        assert!(parse_chid_mappings(&data).is_err());
    }

    #[test]
    fn test_string_offset_outside_data() {
        let mut data = Vec::new();
        push_mapping(
            &mut data,
            ChidMappingHeader {
                descriptor: 0x1000_001c,
                chid: Guid::default(),
            },
            ChidMappingDeviceTree {
                name_offset: 0x0fff_ffff, // invalid offset
                compatible_offset: 0,
            }
            .as_bytes(),
        ); // type 1, length 28
        push_mapping(&mut data, ChidMappingHeader::default(), &[]); // terminator
        assert!(parse_chid_mappings(&data).is_err());
    }

    // Happy cases

    #[test]
    fn test_empty_terminator() {
        let mut data = Vec::new();
        push_mapping(&mut data, ChidMappingHeader::default(), &[]); // terminator
        let r = parse_chid_mappings(&data).expect("failed to parse CHID mappings");
        assert!(r.is_empty());
    }

    #[test]
    fn test_single_device_tree_mapping() {
        let mut data = Vec::new();
        push_mapping(
            &mut data,
            ChidMappingHeader {
                descriptor: 0x1000_001c,
                chid: Guid::default(),
            },
            ChidMappingDeviceTree {
                name_offset: 48,
                compatible_offset: 57,
            }
            .as_bytes(),
        ); // type 1, length 28
        push_mapping(&mut data, ChidMappingHeader::default(), &[]); // terminator
        push_strings(&mut data, &["MyDevice", "my,compatible"]);
        let r = parse_chid_mappings(&data).expect("failed to parse CHID mappings");
        assert_eq!(
            r,
            vec![ChidMapping::DeviceTree {
                chid: Guid::default(),
                name: Some("MyDevice"),
                compatible: Some("my,compatible"),
            }]
        );
    }

    #[test]
    fn test_multiple_mappings() {
        let mut data = Vec::new();
        push_mapping(
            &mut data,
            ChidMappingHeader {
                descriptor: 0x1000_001c,
                chid: Guid::default(),
            },
            ChidMappingDeviceTree {
                name_offset: 276,
                compatible_offset: 285,
            }
            .as_bytes(),
        ); // type 1, length 28
        push_mapping(
            &mut data,
            ChidMappingHeader {
                descriptor: 0x5000_00c8,
                chid: Guid::default(),
            },
            &[0x42u8; 180],
        ); // unknown type 5, length 200, skipped
        push_mapping(
            &mut data,
            ChidMappingHeader {
                descriptor: 0x2000_001c,
                chid: Guid::read_from_bytes(&[1u8; 16]).unwrap(),
            },
            ChidMappingUefiFw {
                name_offset: 299,
                fwid_offset: 310,
            }
            .as_bytes(),
        ); // type 2, length 28
        push_mapping(&mut data, ChidMappingHeader::default(), &[]); // terminator
        push_strings(
            &mut data,
            &["MyDevice", "my,compatible", "MyFirmware", "FWID1234"],
        );
        let r = parse_chid_mappings(&data).expect("failed to parse CHID mappings");
        assert_eq!(
            r,
            vec![
                ChidMapping::DeviceTree {
                    chid: Guid::default(),
                    name: Some("MyDevice"),
                    compatible: Some("my,compatible"),
                },
                ChidMapping::Unknown {
                    kind: 5,
                    chid: Guid::default(),
                    body: &[0x42u8; 180],
                },
                ChidMapping::UefiFw {
                    chid: Guid::read_from_bytes(&[1u8; 16]).unwrap(),
                    name: Some("MyFirmware"),
                    fwid: Some("FWID1234"),
                },
            ]
        );
    }

    #[test]
    fn test_real_mappings_from_ubuntu_kernel() {
        use ChidMapping::*;
        let bytes = include_bytes!("../testdata/linux_signed_6.17.0-6.6_arm64_hwids.bin");
        let r = parse_chid_mappings(bytes).expect("failed to parse CHID mappings");
        let e = [
            DeviceTree {
                chid: guid_str("08b75d1f-6643-52a1-9bdd-071052860b33"),
                name: Some("LENOVO Miix 630"),
                compatible: Some("lenovo,miix-630"),
            },
            DeviceTree {
                chid: guid_str("14f581d2-d059-5cb2-9f8b-56d8be7932c9"),
                name: Some("LENOVO Miix 630"),
                compatible: Some("lenovo,miix-630"),
            },
            DeviceTree {
                chid: guid_str("16a55446-eba9-5f97-80e3-5e39d8209bc3"),
                name: Some("LENOVO Miix 630"),
                compatible: Some("lenovo,miix-630"),
            },
            DeviceTree {
                chid: guid_str("307ab358-ed84-57fe-bf05-e9195a28198d"),
                name: Some("LENOVO Miix 630"),
                compatible: Some("lenovo,miix-630"),
            },
            DeviceTree {
                chid: guid_str("34df58d6-b605-50aa-9313-9b34f5c4b6fc"),
                name: Some("LENOVO Miix 630"),
                compatible: Some("lenovo,miix-630"),
            },
            DeviceTree {
                chid: guid_str("7e613574-5445-5797-9567-2d0ed86e6ffa"),
                name: Some("LENOVO Miix 630"),
                compatible: Some("lenovo,miix-630"),
            },
            DeviceTree {
                chid: guid_str("a51054fb-5eef-594a-a5a0-cd87632d0aea"),
                name: Some("LENOVO Miix 630"),
                compatible: Some("lenovo,miix-630"),
            },
            DeviceTree {
                chid: guid_str("b0f4463c-f851-5ec3-b031-2ccb873a609a"),
                name: Some("LENOVO Miix 630"),
                compatible: Some("lenovo,miix-630"),
            },
            DeviceTree {
                chid: guid_str("c4c9a6be-5383-5de7-af35-c2de505edec8"),
                name: Some("LENOVO Miix 630"),
                compatible: Some("lenovo,miix-630"),
            },
            DeviceTree {
                chid: guid_str("e0a96696-f0a6-5466-a6db-207fbe8bae3c"),
                name: Some("LENOVO Miix 630"),
                compatible: Some("lenovo,miix-630"),
            },
            DeviceTree {
                chid: guid_str("175f000b-3d05-5c01-aedd-817b1a141f93"),
                name: Some("Acer Aspire 1"),
                compatible: Some("acer,aspire1"),
            },
            DeviceTree {
                chid: guid_str("260192d4-06d4-5124-ab46-ba210f4c14d7"),
                name: Some("Acer Aspire 1"),
                compatible: Some("acer,aspire1"),
            },
            DeviceTree {
                chid: guid_str("373bfde5-ffaa-504c-84f3-f8f5357dfc29"),
                name: Some("Acer Aspire 1"),
                compatible: Some("acer,aspire1"),
            },
            DeviceTree {
                chid: guid_str("45d37dbe-40fb-57bd-a257-55f422d4dc0a"),
                name: Some("Acer Aspire 1"),
                compatible: Some("acer,aspire1"),
            },
            DeviceTree {
                chid: guid_str("68b38fff-aadc-512c-937b-99d9c13eb484"),
                name: Some("Acer Aspire 1"),
                compatible: Some("acer,aspire1"),
            },
            DeviceTree {
                chid: guid_str("7c107a7f-2d77-51aa-aef8-8d777e26ffbc"),
                name: Some("Acer Aspire 1"),
                compatible: Some("acer,aspire1"),
            },
            DeviceTree {
                chid: guid_str("7e15f49e-04b4-5d56-a567-e7a15ba2aca1"),
                name: Some("Acer Aspire 1"),
                compatible: Some("acer,aspire1"),
            },
            DeviceTree {
                chid: guid_str("82fe1869-361c-56b2-b853-631747e64aa7"),
                name: Some("Acer Aspire 1"),
                compatible: Some("acer,aspire1"),
            },
            DeviceTree {
                chid: guid_str("965e3681-de3b-5e39-bb62-7d4917d7e36f"),
                name: Some("Acer Aspire 1"),
                compatible: Some("acer,aspire1"),
            },
            DeviceTree {
                chid: guid_str("e12521bf-0ed8-5406-af87-adad812c57c5"),
                name: Some("Acer Aspire 1"),
                compatible: Some("acer,aspire1"),
            },
            DeviceTree {
                chid: guid_str("faa12ed4-bd49-5471-8f74-75c2267c3b46"),
                name: Some("Acer Aspire 1"),
                compatible: Some("acer,aspire1"),
            },
            DeviceTree {
                chid: guid_str("01439aea-e75c-5fbb-8842-18dcd1a7b8b3"),
                name: Some("LENOVO Yoga 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("5100eeed-c5e2-5b74-9c24-a22ca0644826"),
                name: Some("LENOVO Yoga 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("566b9ae8-a7fd-5c44-94d6-bac3e4cf38a7"),
                name: Some("LENOVO Yoga 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("65ab9f32-bbc8-52d3-87f9-b618fda7c07e"),
                name: Some("LENOVO Yoga 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("6f3bdfb7-f832-5c5f-9777-9e3db35e22a6"),
                name: Some("LENOVO Yoga 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("7e7007ac-603c-55ef-bb77-3548784b9578"),
                name: Some("LENOVO Yoga 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("a1a13249-2689-5c6d-a43f-98af040284c4"),
                name: Some("LENOVO Yoga 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("c4ea686c-c56c-5e8e-a91e-89056683d417"),
                name: Some("LENOVO Yoga 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("ddb3bcda-db7b-579d-9dd9-bcc4f5b052b8"),
                name: Some("LENOVO Yoga 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("ea646c11-3da1-5c8d-9346-8ff156746650"),
                name: Some("LENOVO Yoga 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("fb364c09-efc0-5d16-ac97-0a3e6235b16c"),
                name: Some("LENOVO Yoga 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("06675172-9a6e-5276-a505-d205688a87f0"),
                name: Some("LENOVO Flex 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("12c0e5b0-8886-5444-b42b-93692fa736df"),
                name: Some("LENOVO Flex 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("16551bd5-37b0-571d-a94c-da61a9cfccf5"),
                name: Some("LENOVO Flex 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("23dcfb84-d132-5f60-878e-64fe0b9417d6"),
                name: Some("LENOVO Flex 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("39fca706-c9a2-54d4-8c7c-d5e292d0a725"),
                name: Some("LENOVO Flex 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("997c1c76-5595-5300-9f58-94d2c6ffc586"),
                name: Some("LENOVO Flex 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("ad47f2e9-2f8c-5cd1-a44e-82f35a43e44e"),
                name: Some("LENOVO Flex 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("b9bf941f-3a32-57da-b609-5fff7fb382cd"),
                name: Some("LENOVO Flex 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("df3ecc56-b61b-5f8e-896f-801a42b536d6"),
                name: Some("LENOVO Flex 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("ea658d2b-f644-555d-9b72-e1642401a795"),
                name: Some("LENOVO Flex 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("fb5c3077-39d5-5a44-97ce-2d3be5f6bfec"),
                name: Some("LENOVO Flex 5G 14Q8CX05"),
                compatible: Some("lenovo,flex-5g"),
            },
            DeviceTree {
                chid: guid_str("0c78ef16-4fe0-5e33-908e-b038949ee608"),
                name: Some("HUAWEI MateBook E"),
                compatible: Some("huawei,gaokun3"),
            },
            DeviceTree {
                chid: guid_str("13311789-793f-5d95-942c-3b6414a8ad1a"),
                name: Some("HUAWEI MateBook E"),
                compatible: Some("huawei,gaokun3"),
            },
            DeviceTree {
                chid: guid_str("1c2effc1-1038-584d-ae8b-7c912c8e9504"),
                name: Some("HUAWEI MateBook E"),
                compatible: Some("huawei,gaokun3"),
            },
            DeviceTree {
                chid: guid_str("3eb6683b-0153-5365-81c6-cc599783e9c7"),
                name: Some("HUAWEI MateBook E"),
                compatible: Some("huawei,gaokun3"),
            },
            DeviceTree {
                chid: guid_str("4f04f31f-17f0-583f-802f-82c3a0b34128"),
                name: Some("HUAWEI MateBook E"),
                compatible: Some("huawei,gaokun3"),
            },
            DeviceTree {
                chid: guid_str("6eb75906-3a4e-5de4-94c5-374d8f9723e5"),
                name: Some("HUAWEI MateBook E"),
                compatible: Some("huawei,gaokun3"),
            },
            DeviceTree {
                chid: guid_str("7ea8b73b-2cbb-562b-aecc-7f0f64c42630"),
                name: Some("HUAWEI MateBook E"),
                compatible: Some("huawei,gaokun3"),
            },
            DeviceTree {
                chid: guid_str("80c86c24-c7a7-5714-abf2-4c48f348cecc"),
                name: Some("HUAWEI MateBook E"),
                compatible: Some("huawei,gaokun3"),
            },
            DeviceTree {
                chid: guid_str("b866fc5c-261b-56d8-99e8-03ea0646af8f"),
                name: Some("HUAWEI MateBook E"),
                compatible: Some("huawei,gaokun3"),
            },
            DeviceTree {
                chid: guid_str("d8846172-f0a0-55ba-bf41-55641f588ea7"),
                name: Some("HUAWEI MateBook E"),
                compatible: Some("huawei,gaokun3"),
            },
            DeviceTree {
                chid: guid_str("e1b94e53-0f20-5d01-abfc-cfb348544a31"),
                name: Some("HUAWEI MateBook E"),
                compatible: Some("huawei,gaokun3"),
            },
            DeviceTree {
                chid: guid_str("e5a0ed2b-7fed-5e2d-94ed-43dbaf0b9ccc"),
                name: Some("HUAWEI MateBook E"),
                compatible: Some("huawei,gaokun3"),
            },
            DeviceTree {
                chid: guid_str("e98c95a8-b50e-5d8b-b2db-c679a39163df"),
                name: Some("HUAWEI MateBook E"),
                compatible: Some("huawei,gaokun3"),
            },
            DeviceTree {
                chid: guid_str("0909a1c3-3a02-59a0-b1ea-04f1449c104f"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("3486eccc-d0ac-534a-9e2f-a1c18bc310c6"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("3ad863ab-0181-5a2f-9cc1-70eedc446da9"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("3f9d2d91-73b2-5316-8c72-a0ecb3f0dae5"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("4b189129-8eb2-585c-a1bb-a4cfc979433a"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("4df470e6-7878-5b0f-b2e0-733d5d9fa228"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("64b71f12-4341-5e5c-b7cd-25b6503799e3"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("69acf6bf-ed33-5806-857f-c76971d7061e"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("69c47e1e-fde2-5062-b777-acbeab73784b"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("810e34c6-cc69-5e36-8675-2f6e354272d3"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("873455fb-b2c5-5c0c-9c2c-90e80d44da57"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("9f47e28f-e1ee-5cb5-b4ce-8f0605752b3d"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("a1dfe209-99e5-5ff2-9922-aa4c11491b49"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("abdbb2cb-ab52-5674-9d0a-2e2cb69bcbb4"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("b265d777-007e-56e5-b0e2-bd666ab867be"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("b41f58ed-7631-561f-9b0c-449a9c293afa"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("b470d002-ad8e-5d5c-a7bf-bb1333f2ce4b"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("c869f39e-f205-5ca0-be7b-d90f90ef5556"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("ddf28a3f-43fc-54a4-a6a7-4cba5ad46b3e"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("ddfbdaa2-7c46-5103-be64-84a9f88c485f"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("f22c935e-2dc8-5949-9486-09bbf10361b2"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("fbf92a11-bb6f-5adb-b5a7-8abf9acbd7d9"),
                name: Some("LENOVO ThinkPad X13s Gen 1"),
                compatible: Some("lenovo,thinkpad-x13s"),
            },
            DeviceTree {
                chid: guid_str("046fefee-341b-5c40-b0a3-1c647d31b500"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,blackrock"),
            },
            DeviceTree {
                chid: guid_str("08f06457-aa19-51c5-be4c-0087ce4fa2ed"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,blackrock"),
            },
            DeviceTree {
                chid: guid_str("11b80238-dbee-57bc-8b26-83c9e5b4057d"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,blackrock"),
            },
            DeviceTree {
                chid: guid_str("53b87f48-fc47-54e9-ade5-f1a95e885681"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,blackrock"),
            },
            DeviceTree {
                chid: guid_str("5bd24fc5-5edb-51f6-82e6-31a9ef954c5b"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,blackrock"),
            },
            DeviceTree {
                chid: guid_str("69ba0503-ca94-5fa3-b78c-5fa21a66c620"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,blackrock"),
            },
            DeviceTree {
                chid: guid_str("813677fa-6d11-5756-a44d-dde0f552d3f6"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,blackrock"),
            },
            DeviceTree {
                chid: guid_str("ad2ee931-a048-5253-b350-98c482670765"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,blackrock"),
            },
            DeviceTree {
                chid: guid_str("ce83144b-b123-59e5-8a9a-0c1a13643fc4"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,blackrock"),
            },
            DeviceTree {
                chid: guid_str("d67e799e-2ba7-555a-a874-a0523a8b3b11"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,blackrock"),
            },
            DeviceTree {
                chid: guid_str("f59639f4-4970-5706-9a75-519dd059f69e"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,blackrock"),
            },
            DeviceTree {
                chid: guid_str("009d2337-4f76-514e-b2c1-b2816447b048"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,arcata"),
            },
            DeviceTree {
                chid: guid_str("3a486e6f-3b0a-5603-a483-503381d3d8c3"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,arcata"),
            },
            DeviceTree {
                chid: guid_str("5caa88bc-ea9b-5d73-a69a-89024bfff854"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,arcata"),
            },
            DeviceTree {
                chid: guid_str("6309fbb9-68f4-54f9-bbc9-b3ca9685b48c"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,arcata"),
            },
            DeviceTree {
                chid: guid_str("636b6071-7848-50d5-b0b5-6290c49e9306"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,arcata"),
            },
            DeviceTree {
                chid: guid_str("94fb24a7-ff7a-5d70-9ac8-518a9e44ea64"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,arcata"),
            },
            DeviceTree {
                chid: guid_str("9bac72c6-83f6-5e21-af8e-bc1f5c2b7cc8"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,arcata"),
            },
            DeviceTree {
                chid: guid_str("9d70dcfd-f56b-58bf-b1bd-a1b8f2b0ec7e"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,arcata"),
            },
            DeviceTree {
                chid: guid_str("a659ee2b-502d-50f7-9921-bdbd34734e0b"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,arcata"),
            },
            DeviceTree {
                chid: guid_str("c0cf7078-c325-5cf6-966b-3bbbc155275b"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,arcata"),
            },
            DeviceTree {
                chid: guid_str("e3d941fa-2bfa-5875-8efd-87ce997f8338"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,arcata"),
            },
            DeviceTree {
                chid: guid_str("30b031c0-9de7-5d31-a61c-dee772871b7d"),
                name: Some("LENOVO YOGA C630-13Q50"),
                compatible: Some("lenovo,yoga-c630"),
            },
            DeviceTree {
                chid: guid_str("382926c0-ce35-53af-8ff9-ca9cc06cfc7b"),
                name: Some("LENOVO YOGA C630-13Q50"),
                compatible: Some("lenovo,yoga-c630"),
            },
            DeviceTree {
                chid: guid_str("43b71948-9c47-5372-a5cb-18db47bb873f"),
                name: Some("LENOVO YOGA C630-13Q50"),
                compatible: Some("lenovo,yoga-c630"),
            },
            DeviceTree {
                chid: guid_str("5ca3cf2b-d6e9-5b54-93f7-1cebd7b3704f"),
                name: Some("LENOVO YOGA C630-13Q50"),
                compatible: Some("lenovo,yoga-c630"),
            },
            DeviceTree {
                chid: guid_str("67a23be6-42a6-5900-8325-847a318ce252"),
                name: Some("LENOVO YOGA C630-13Q50"),
                compatible: Some("lenovo,yoga-c630"),
            },
            DeviceTree {
                chid: guid_str("81f308c0-db65-50c2-a660-52e06fc0ff9f"),
                name: Some("LENOVO YOGA C630-13Q50"),
                compatible: Some("lenovo,yoga-c630"),
            },
            DeviceTree {
                chid: guid_str("8f56cf17-7bdd-5414-832d-97cd26837114"),
                name: Some("LENOVO YOGA C630-13Q50"),
                compatible: Some("lenovo,yoga-c630"),
            },
            DeviceTree {
                chid: guid_str("94f73d29-3981-59a8-8f25-214f84d1522a"),
                name: Some("LENOVO YOGA C630-13Q50"),
                compatible: Some("lenovo,yoga-c630"),
            },
            DeviceTree {
                chid: guid_str("b323d38a-88c6-5cf6-af0d-0db3f3c2560d"),
                name: Some("LENOVO YOGA C630-13Q50"),
                compatible: Some("lenovo,yoga-c630"),
            },
            DeviceTree {
                chid: guid_str("b8c71349-3669-56f3-99ee-ae473a2edd96"),
                name: Some("LENOVO YOGA C630-13Q50"),
                compatible: Some("lenovo,yoga-c630"),
            },
            DeviceTree {
                chid: guid_str("d17c132e-f06e-5e38-8084-9cd642dd9b34"),
                name: Some("LENOVO YOGA C630-13Q50"),
                compatible: Some("lenovo,yoga-c630"),
            },
            DeviceTree {
                chid: guid_str("4bb05d50-6c4f-525d-a9ec-8924afd6edea"),
                name: Some("Qualcomm SCP_HAMOA"),
                compatible: Some("qcom,x1e001de-devkit"),
            },
            DeviceTree {
                chid: guid_str("830bd4a2-2498-55cf-b561-48f7dc5f4820"),
                name: Some("Qualcomm SCP_HAMOA"),
                compatible: Some("qcom,x1e001de-devkit"),
            },
            DeviceTree {
                chid: guid_str("9cba20d0-17ad-559f-94cd-cfcbbf5f71f5"),
                name: Some("Qualcomm SCP_HAMOA"),
                compatible: Some("qcom,x1e001de-devkit"),
            },
            DeviceTree {
                chid: guid_str("baa7a649-12d8-56c7-93c5-a4e10f4852be"),
                name: Some("Qualcomm SCP_HAMOA"),
                compatible: Some("qcom,x1e001de-devkit"),
            },
            DeviceTree {
                chid: guid_str("c8e75ab8-555c-5952-a3e3-5b607bea031d"),
                name: Some("Qualcomm SCP_HAMOA"),
                compatible: Some("qcom,x1e001de-devkit"),
            },
            DeviceTree {
                chid: guid_str("f37dc44b-0be4-5a70-86bd-81f3dacff2e9"),
                name: Some("Qualcomm SCP_HAMOA"),
                compatible: Some("qcom,x1e001de-devkit"),
            },
            DeviceTree {
                chid: guid_str("1480f3ca-b01a-5d7c-bbc9-7a17d7b4b58d"),
                name: Some("LENOVO ThinkPad T14s Gen 6 (LCD)"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("1538c7fb-26b6-5144-b16f-2500b5a0a503"),
                name: Some("LENOVO ThinkPad T14s Gen 6 (LCD)"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("2b1b6e68-cee9-549b-b8ae-10c274b8c3a6"),
                name: Some("LENOVO ThinkPad T14s Gen 6 (LCD)"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("48a732b5-3989-5ac3-b661-516a46f00792"),
                name: Some("LENOVO ThinkPad T14s Gen 6 (LCD)"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("53b6927f-d6ca-5674-a0e8-a50a989d4ba0"),
                name: Some("LENOVO ThinkPad T14s Gen 6 (LCD)"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("83579fdf-8faf-57b4-b265-d2e817c7cf3f"),
                name: Some("LENOVO ThinkPad T14s Gen 6 (LCD)"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("a07b8e34-d6b6-58b2-9963-38216ec67159"),
                name: Some("LENOVO ThinkPad T14s Gen 6 (LCD)"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("a8852b56-45ea-5377-ba2d-1910a5c897bb"),
                name: Some("LENOVO ThinkPad T14s Gen 6 (LCD)"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("b0e14398-0a96-5736-840b-7349e5c0b85c"),
                name: Some("LENOVO ThinkPad T14s Gen 6 (LCD)"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("08ba7f5b-8136-5938-835a-fd99143d34a5"),
                name: Some("LENOVO ThinkPad T14s Gen 6 (OLED)"),
                compatible: Some("lenovo,thinkpad-t14s-oled"),
            },
            DeviceTree {
                chid: guid_str("27378ce5-d999-5c3b-acde-0404805afd3b"),
                name: Some("LENOVO ThinkPad T14s Gen 6 (OLED)"),
                compatible: Some("lenovo,thinkpad-t14s-oled"),
            },
            DeviceTree {
                chid: guid_str("77ffaabe-038a-550f-b6ea-485dc49d4b45"),
                name: Some("LENOVO ThinkPad T14s Gen 6 (OLED)"),
                compatible: Some("lenovo,thinkpad-t14s-oled"),
            },
            DeviceTree {
                chid: guid_str("7c09107d-0ac3-5582-837f-c614d518cf62"),
                name: Some("LENOVO ThinkPad T14s Gen 6 (OLED)"),
                compatible: Some("lenovo,thinkpad-t14s-oled"),
            },
            DeviceTree {
                chid: guid_str("c012b92d-a6a6-57fa-ba06-4f3062d891d4"),
                name: Some("LENOVO ThinkPad T14s Gen 6 (OLED)"),
                compatible: Some("lenovo,thinkpad-t14s-oled"),
            },
            DeviceTree {
                chid: guid_str("e78c4e7a-68c3-5b29-b2a0-dbd2785e28cd"),
                name: Some("LENOVO ThinkPad T14s Gen 6 (OLED)"),
                compatible: Some("lenovo,thinkpad-t14s-oled"),
            },
            DeviceTree {
                chid: guid_str("19b622ef-27fe-5c2e-bc53-13a79b862c65"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("1d9f3ebb-96de-5dd6-8c88-38308b0c1c44"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("34e7fadd-9c7d-5f91-ba7f-cedb04d59b9a"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("498d60ae-9b1d-5b67-8abd-af571babfa94"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("513976f8-3f51-5b42-9ae0-931ce23c5f38"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("5180bc01-5d18-5870-b955-969da38b2647"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("578dd7d5-5871-5bd5-92a9-be07f1067b92"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("5c20e964-d530-5dd7-9efd-4aed9e73c3cb"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("74593764-b6b9-58e9-bedc-93ebbb1eb057"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("76032e78-67a8-5dab-8512-157bfcfb8f75"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("791ecd9d-1547-58e6-b72a-5ce417b729dd"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("82fa4a02-8c3c-55f9-b0c9-e8feb669fd3a"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("86a0d770-3ca1-57fa-ac05-413481c00a24"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("8c602147-5363-5374-859e-8b7fe2d4d3ce"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("8cfd85bb-0d77-59df-8546-264239be475e"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("a20ae3ec-49a1-5cb5-acb8-5d31c77b105a"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("a5a4e3c1-5922-5ed6-b78e-9f0ea873a988"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("a9b59fea-e841-508a-a245-3a2d8d2802de"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("ac09e50f-9b3b-53c0-9752-377c3a0baaa0"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("acbac5af-aa6a-5690-88f3-e910f04a7ead"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("c81fee2f-cf41-5d5a-8c7b-afd6585b1d81"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("d93b21c0-5ed9-5955-911a-5b15f114d786"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("dd83478e-e01b-5631-ae74-92ae275a9b4e"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("e5d83424-0ecb-5632-b7b1-500f04e82725"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("ed647f93-3075-598b-9d89-d0f30ec11707"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("f6cd4a9f-9632-516e-b748-65952f7380c5"),
                name: Some("LENOVO ThinkPad T14s Gen 6"),
                compatible: Some("lenovo,thinkpad-t14s-lcd"),
            },
            DeviceTree {
                chid: guid_str("137a5f94-8fcf-5581-8ac6-70d50fdba4a6"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Vivobook S 15"),
                compatible: Some("asus,vivobook-s15"),
            },
            DeviceTree {
                chid: guid_str("1b6a0689-3f70-57e0-8bf3-39a8a74213e8"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Vivobook S 15"),
                compatible: Some("asus,vivobook-s15"),
            },
            DeviceTree {
                chid: guid_str("3a3ef092-d5f1-5d4d-acea-70b38ef56e53"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Vivobook S 15"),
                compatible: Some("asus,vivobook-s15"),
            },
            DeviceTree {
                chid: guid_str("4262e277-58d3-5ac4-9858-c0751ad06f5c"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Vivobook S 15"),
                compatible: Some("asus,vivobook-s15"),
            },
            DeviceTree {
                chid: guid_str("6d634332-21fc-57c8-bc6b-e0f800f69f95"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Vivobook S 15"),
                compatible: Some("asus,vivobook-s15"),
            },
            DeviceTree {
                chid: guid_str("80430e03-90f0-5355-84b2-28fb17367203"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Vivobook S 15"),
                compatible: Some("asus,vivobook-s15"),
            },
            DeviceTree {
                chid: guid_str("807fe49f-cfd2-537d-b635-47bec9e36baf"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Vivobook S 15"),
                compatible: Some("asus,vivobook-s15"),
            },
            DeviceTree {
                chid: guid_str("a6debedb-f954-5aa1-8260-4dc3b567c95f"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Vivobook S 15"),
                compatible: Some("asus,vivobook-s15"),
            },
            DeviceTree {
                chid: guid_str("c71e903b-4255-56cb-b961-a8f87b452cbe"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Vivobook S 15"),
                compatible: Some("asus,vivobook-s15"),
            },
            DeviceTree {
                chid: guid_str("d0fce8d6-a709-5bf0-8be0-6ac6ab44b8e0"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Vivobook S 15"),
                compatible: Some("asus,vivobook-s15"),
            },
            DeviceTree {
                chid: guid_str("f3a6ca3e-4791-5bb0-915e-0b31856ec19c"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Vivobook S 15"),
                compatible: Some("asus,vivobook-s15"),
            },
            DeviceTree {
                chid: guid_str("f54cd4e6-3666-5b56-abd3-a5f2df50c534"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Vivobook S 15"),
                compatible: Some("asus,vivobook-s15"),
            },
            DeviceTree {
                chid: guid_str("fa342b0a-9e22-541f-8e95-93106778f97d"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Vivobook S 15"),
                compatible: Some("asus,vivobook-s15"),
            },
            DeviceTree {
                chid: guid_str("91971b38-ae5d-5e14-9f44-7c0316710593"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407qa-oled"),
            },
            DeviceTree {
                chid: guid_str("f84ba711-7075-5c1b-a03c-57d2521a1ac2"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407qa-oled"),
            },
            DeviceTree {
                chid: guid_str("0bfddfaf-e393-5dfe-a805-39b8b1098c81"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407ra"),
            },
            DeviceTree {
                chid: guid_str("0c3f5e9c-eddb-5ba2-88ee-06ae0221a53d"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407ra"),
            },
            DeviceTree {
                chid: guid_str("1f2f1045-a811-5e42-b31e-b433e384fc79"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407ra"),
            },
            DeviceTree {
                chid: guid_str("20b8b77d-e450-550b-b1ff-55d3317f59a6"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407ra"),
            },
            DeviceTree {
                chid: guid_str("24652d54-00f4-59ae-96fb-f7adbfa4a939"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407ra"),
            },
            DeviceTree {
                chid: guid_str("2d610a5e-ef69-5e60-b15e-7786d0ebd79e"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407ra"),
            },
            DeviceTree {
                chid: guid_str("3884ad58-4d63-589a-be98-b8ab1ddf3b93"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407ra"),
            },
            DeviceTree {
                chid: guid_str("5c9fc73f-f915-52bf-a82d-9c7fe2274ecc"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407ra"),
            },
            DeviceTree {
                chid: guid_str("6f892377-a51e-5f99-a363-b79f28fc55f9"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407ra"),
            },
            DeviceTree {
                chid: guid_str("b307ab54-c79d-58ca-a3b2-d1b1e325bfc3"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407ra"),
            },
            DeviceTree {
                chid: guid_str("c41d2cda-fda7-522c-b1a7-4a835c15c43d"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407ra"),
            },
            DeviceTree {
                chid: guid_str("cedbcc19-3a5a-5bae-9973-f8e158188de7"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407ra"),
            },
            DeviceTree {
                chid: guid_str("f0bb1cd4-995a-5c90-946b-9bb958f35f42"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407ra"),
            },
            DeviceTree {
                chid: guid_str("2405af0b-d21d-5196-a228-4acffe7b3a10"),
                name: Some("Qualcomm SCP_HAMOA"),
                compatible: Some("qcom,x1e80100-crd"),
            },
            DeviceTree {
                chid: guid_str("339fc6d2-e0f4-5226-9dd9-62c4dc41881d"),
                name: Some("Qualcomm SCP_HAMOA"),
                compatible: Some("qcom,x1e80100-crd"),
            },
            DeviceTree {
                chid: guid_str("361b3d63-be90-52c2-8798-a05fbd68b773"),
                name: Some("Qualcomm SCP_HAMOA"),
                compatible: Some("qcom,x1e80100-crd"),
            },
            DeviceTree {
                chid: guid_str("4bb05d50-6c4f-525d-a9ec-8924afd6edea"),
                name: Some("Qualcomm SCP_HAMOA"),
                compatible: Some("qcom,x1e80100-crd"),
            },
            DeviceTree {
                chid: guid_str("7faef667-9eb2-53f4-9764-26fe0e92fbff"),
                name: Some("Qualcomm SCP_HAMOA"),
                compatible: Some("qcom,x1e80100-crd"),
            },
            DeviceTree {
                chid: guid_str("8fa88c58-23eb-5aea-9ea7-c4a98ded7352"),
                name: Some("Qualcomm SCP_HAMOA"),
                compatible: Some("qcom,x1e80100-crd"),
            },
            DeviceTree {
                chid: guid_str("b6d4eee8-30f3-564a-8246-e83935cf8dbb"),
                name: Some("Qualcomm SCP_HAMOA"),
                compatible: Some("qcom,x1e80100-crd"),
            },
            DeviceTree {
                chid: guid_str("d52e3fb6-202c-5cfa-a27c-e3ffe15339fb"),
                name: Some("Qualcomm SCP_HAMOA"),
                compatible: Some("qcom,x1e80100-crd"),
            },
            DeviceTree {
                chid: guid_str("e73870a5-90e8-528d-93fd-3da59f78df18"),
                name: Some("Qualcomm SCP_HAMOA"),
                compatible: Some("qcom,x1e80100-crd"),
            },
            DeviceTree {
                chid: guid_str("07c6477a-7ef7-56c7-91d9-73d23295b0c0"),
                name: Some("Dell Inc. Inspiron"),
                compatible: Some("dell,inspiron-14-plus-7441"),
            },
            DeviceTree {
                chid: guid_str("07c6bbd9-caf8-5025-84d8-4efdb790f663"),
                name: Some("Dell Inc. Inspiron"),
                compatible: Some("dell,inspiron-14-plus-7441"),
            },
            DeviceTree {
                chid: guid_str("0d9ce3bc-620c-5209-9e82-7382cb6cffcd"),
                name: Some("Dell Inc. Inspiron"),
                compatible: Some("dell,inspiron-14-plus-7441"),
            },
            DeviceTree {
                chid: guid_str("4689ccaf-4f31-5146-887f-ca965da0f28a"),
                name: Some("Dell Inc. Inspiron"),
                compatible: Some("dell,inspiron-14-plus-7441"),
            },
            DeviceTree {
                chid: guid_str("8548ce7c-fdf3-55d0-95ba-606ca8db50da"),
                name: Some("Dell Inc. Inspiron"),
                compatible: Some("dell,inspiron-14-plus-7441"),
            },
            DeviceTree {
                chid: guid_str("90117b25-1646-515b-bfeb-286e74f2a1e8"),
                name: Some("Dell Inc. Inspiron"),
                compatible: Some("dell,inspiron-14-plus-7441"),
            },
            DeviceTree {
                chid: guid_str("a66b1244-0027-5451-a96a-dfcfc42ab892"),
                name: Some("Dell Inc. Inspiron"),
                compatible: Some("dell,inspiron-14-plus-7441"),
            },
            DeviceTree {
                chid: guid_str("bea2e67a-b660-5044-8acd-0d28e8c2e974"),
                name: Some("Dell Inc. Inspiron"),
                compatible: Some("dell,inspiron-14-plus-7441"),
            },
            DeviceTree {
                chid: guid_str("c1190be1-8ed5-50f1-9097-2a73ad9c4eb1"),
                name: Some("Dell Inc. Inspiron"),
                compatible: Some("dell,inspiron-14-plus-7441"),
            },
            DeviceTree {
                chid: guid_str("e4c9fe83-73ba-5160-bc18-57d4a98e960d"),
                name: Some("Dell Inc. Inspiron"),
                compatible: Some("dell,inspiron-14-plus-7441"),
            },
            DeviceTree {
                chid: guid_str("ef84110e-bd09-5f0f-a3dd-7995b4a3a706"),
                name: Some("Dell Inc. Inspiron"),
                compatible: Some("dell,inspiron-14-plus-7441"),
            },
            DeviceTree {
                chid: guid_str("efc06900-f603-5944-88e5-4de722816f91"),
                name: Some("Dell Inc. Inspiron"),
                compatible: Some("dell,inspiron-14-plus-7441"),
            },
            DeviceTree {
                chid: guid_str("ff394805-0b5f-52f6-9e1d-afb1d2bee411"),
                name: Some("Dell Inc. Inspiron"),
                compatible: Some("dell,inspiron-14-plus-7441"),
            },
            DeviceTree {
                chid: guid_str("2b9277cd-85b1-51ee-9a38-d477632532da"),
                name: Some("Dell Inc. Latitude"),
                compatible: Some("dell,latitude-7455"),
            },
            DeviceTree {
                chid: guid_str("4f73f73b-e639-5353-bbf7-d851e48f18fc"),
                name: Some("Dell Inc. Latitude"),
                compatible: Some("dell,latitude-7455"),
            },
            DeviceTree {
                chid: guid_str("59e5c810-9e60-5a89-8665-db36c56b34d6"),
                name: Some("Dell Inc. Latitude"),
                compatible: Some("dell,latitude-7455"),
            },
            DeviceTree {
                chid: guid_str("683e4579-8440-5bc1-89ac-dfcd7c25b307"),
                name: Some("Dell Inc. Latitude"),
                compatible: Some("dell,latitude-7455"),
            },
            DeviceTree {
                chid: guid_str("68822228-a3e0-5b12-942d-9408751405d1"),
                name: Some("Dell Inc. Latitude"),
                compatible: Some("dell,latitude-7455"),
            },
            DeviceTree {
                chid: guid_str("6e01222b-b2aa-531e-b95f-0e4b2a063364"),
                name: Some("Dell Inc. Latitude"),
                compatible: Some("dell,latitude-7455"),
            },
            DeviceTree {
                chid: guid_str("903e3a6f-e14b-5643-9d55-244f917aadb6"),
                name: Some("Dell Inc. Latitude"),
                compatible: Some("dell,latitude-7455"),
            },
            DeviceTree {
                chid: guid_str("93055898-8c85-50e7-adde-8115f194579a"),
                name: Some("Dell Inc. Latitude"),
                compatible: Some("dell,latitude-7455"),
            },
            DeviceTree {
                chid: guid_str("b3e5b59d-84ae-597d-9222-8a4d48480bc3"),
                name: Some("Dell Inc. Latitude"),
                compatible: Some("dell,latitude-7455"),
            },
            DeviceTree {
                chid: guid_str("fb7493ec-9634-5c5a-9f26-69cbf9b92460"),
                name: Some("Dell Inc. Latitude"),
                compatible: Some("dell,latitude-7455"),
            },
            DeviceTree {
                chid: guid_str("1d1baf60-e2f3-5821-9d98-19a131bf8d93"),
                name: Some("Dell Inc. XPS"),
                compatible: Some("dell,xps13-9345"),
            },
            DeviceTree {
                chid: guid_str("1eb87d70-2f37-5f18-85de-30e46c17d540"),
                name: Some("Dell Inc. XPS"),
                compatible: Some("dell,xps13-9345"),
            },
            DeviceTree {
                chid: guid_str("36e8dd88-512d-5a74-86a4-039333f9e15a"),
                name: Some("Dell Inc. XPS"),
                compatible: Some("dell,xps13-9345"),
            },
            DeviceTree {
                chid: guid_str("3b9a1d76-f2e8-52e8-84de-14c5942b3d41"),
                name: Some("Dell Inc. XPS"),
                compatible: Some("dell,xps13-9345"),
            },
            DeviceTree {
                chid: guid_str("3c2649a7-2275-5130-a0c4-cc5f9809a2c1"),
                name: Some("Dell Inc. XPS"),
                compatible: Some("dell,xps13-9345"),
            },
            DeviceTree {
                chid: guid_str("7c7c2920-cb59-56ad-bc8a-939e803b0192"),
                name: Some("Dell Inc. XPS"),
                compatible: Some("dell,xps13-9345"),
            },
            DeviceTree {
                chid: guid_str("81972cb8-6fc7-5e08-b140-b0063ed4fefa"),
                name: Some("Dell Inc. XPS"),
                compatible: Some("dell,xps13-9345"),
            },
            DeviceTree {
                chid: guid_str("940c6349-f0a5-54ba-8deb-10e709e0b76c"),
                name: Some("Dell Inc. XPS"),
                compatible: Some("dell,xps13-9345"),
            },
            DeviceTree {
                chid: guid_str("bc685cec-e979-5cb9-bf02-e15586c7cb4b"),
                name: Some("Dell Inc. XPS"),
                compatible: Some("dell,xps13-9345"),
            },
            DeviceTree {
                chid: guid_str("e656b5f2-69c3-55da-bf22-4dd58d5f6d4f"),
                name: Some("Dell Inc. XPS"),
                compatible: Some("dell,xps13-9345"),
            },
            DeviceTree {
                chid: guid_str("eedeb5d9-1a0e-56e6-9137-eb6a723e58d1"),
                name: Some("Dell Inc. XPS"),
                compatible: Some("dell,xps13-9345"),
            },
            DeviceTree {
                chid: guid_str("07be634a-0442-51fb-8a77-ecb370b5262b"),
                name: Some("HP 103C_5336AN HP EliteBook Ultra"),
                compatible: Some("hp,elitebook-ultra-g1q"),
            },
            DeviceTree {
                chid: guid_str("25f49237-b3b4-58b2-9e58-6cc379781901"),
                name: Some("HP 103C_5336AN HP EliteBook Ultra"),
                compatible: Some("hp,elitebook-ultra-g1q"),
            },
            DeviceTree {
                chid: guid_str("5275e89b-e839-589e-9e90-e1bd6854255b"),
                name: Some("HP 103C_5336AN HP EliteBook Ultra"),
                compatible: Some("hp,elitebook-ultra-g1q"),
            },
            DeviceTree {
                chid: guid_str("5473ba61-2807-585b-b2ac-f0366d84bdc0"),
                name: Some("HP 103C_5336AN HP EliteBook Ultra"),
                compatible: Some("hp,elitebook-ultra-g1q"),
            },
            DeviceTree {
                chid: guid_str("72a81f9e-c30b-5bf0-8449-948f8e593e92"),
                name: Some("HP 103C_5336AN HP EliteBook Ultra"),
                compatible: Some("hp,elitebook-ultra-g1q"),
            },
            DeviceTree {
                chid: guid_str("7bfc4a0e-15c1-58b3-b27b-4e5f60d4397e"),
                name: Some("HP 103C_5336AN HP EliteBook Ultra"),
                compatible: Some("hp,elitebook-ultra-g1q"),
            },
            DeviceTree {
                chid: guid_str("829d699c-f082-5835-967a-8e7022cb20b5"),
                name: Some("HP 103C_5336AN HP EliteBook Ultra"),
                compatible: Some("hp,elitebook-ultra-g1q"),
            },
            DeviceTree {
                chid: guid_str("9145c311-f1b1-5f0f-902d-f6828baf8c46"),
                name: Some("HP 103C_5336AN HP EliteBook Ultra"),
                compatible: Some("hp,elitebook-ultra-g1q"),
            },
            DeviceTree {
                chid: guid_str("a3c9ef57-67ab-5f75-a4cb-48cb7475b025"),
                name: Some("HP 103C_5336AN HP EliteBook Ultra"),
                compatible: Some("hp,elitebook-ultra-g1q"),
            },
            DeviceTree {
                chid: guid_str("b7f376e9-f5f8-5718-b00e-6bd7c265aeab"),
                name: Some("HP 103C_5336AN HP EliteBook Ultra"),
                compatible: Some("hp,elitebook-ultra-g1q"),
            },
            DeviceTree {
                chid: guid_str("ba2ddd3d-a06b-5b1f-9b2a-ce58f748486c"),
                name: Some("HP 103C_5336AN HP EliteBook Ultra"),
                compatible: Some("hp,elitebook-ultra-g1q"),
            },
            DeviceTree {
                chid: guid_str("e4944bcc-a1c3-540e-b400-cd91953b7ba9"),
                name: Some("HP 103C_5336AN HP EliteBook Ultra"),
                compatible: Some("hp,elitebook-ultra-g1q"),
            },
            DeviceTree {
                chid: guid_str("ec4cdb9c-e6ff-581c-ac22-d597b5e880a2"),
                name: Some("HP 103C_5336AN HP EliteBook Ultra"),
                compatible: Some("hp,elitebook-ultra-g1q"),
            },
            DeviceTree {
                chid: guid_str("045dfd0f-068b-5e57-86bd-f41b4b906006"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14"),
            },
            DeviceTree {
                chid: guid_str("1a192aee-2cfd-5ab5-95f9-8093218a48ef"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14"),
            },
            DeviceTree {
                chid: guid_str("54500b82-f7ae-592d-ae68-8c8e362a1475"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14"),
            },
            DeviceTree {
                chid: guid_str("68d24be5-01b6-5d88-83fb-df2bcfa879aa"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14"),
            },
            DeviceTree {
                chid: guid_str("6a4511bc-0a3b-5b10-9c8b-dbcb834ecd83"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14"),
            },
            DeviceTree {
                chid: guid_str("811126e6-4aee-5f9e-827d-d0f12f6a6f00"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14"),
            },
            DeviceTree {
                chid: guid_str("848aeb1d-302b-5b6b-9109-0f4632535915"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14"),
            },
            DeviceTree {
                chid: guid_str("abb2ffec-2acd-5750-8dfd-c3845fd4bf2a"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14"),
            },
            DeviceTree {
                chid: guid_str("ca5dab4f-a301-53c6-b753-c2db56172e0a"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14"),
            },
            DeviceTree {
                chid: guid_str("eaab52c6-ed22-5e1b-b788-fc5a0531291d"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14"),
            },
            DeviceTree {
                chid: guid_str("0700776d-0de7-5ea7-b9bf-77e0454d35e1"),
                name: Some("LENOVO Yoga Slim 7 14Q8X9"),
                compatible: Some("lenovo,yoga-slim7x"),
            },
            DeviceTree {
                chid: guid_str("3fb1e5ba-05cd-5153-ad64-1d8bc6dc7a1b"),
                name: Some("LENOVO Yoga Slim 7 14Q8X9"),
                compatible: Some("lenovo,yoga-slim7x"),
            },
            DeviceTree {
                chid: guid_str("63429d43-c970-570d-aaa7-54300924e0c5"),
                name: Some("LENOVO Yoga Slim 7 14Q8X9"),
                compatible: Some("lenovo,yoga-slim7x"),
            },
            DeviceTree {
                chid: guid_str("6d53c38f-6adb-578b-a418-2abda4d8485d"),
                name: Some("LENOVO Yoga Slim 7 14Q8X9"),
                compatible: Some("lenovo,yoga-slim7x"),
            },
            DeviceTree {
                chid: guid_str("8073dbed-501f-5f5e-a619-4cdd9c00e865"),
                name: Some("LENOVO Yoga Slim 7 14Q8X9"),
                compatible: Some("lenovo,yoga-slim7x"),
            },
            DeviceTree {
                chid: guid_str("8477f828-512b-56cf-af55-c711a6831551"),
                name: Some("LENOVO Yoga Slim 7 14Q8X9"),
                compatible: Some("lenovo,yoga-slim7x"),
            },
            DeviceTree {
                chid: guid_str("d27cf20e-e185-578e-bd46-f4cc3a718bb2"),
                name: Some("LENOVO Yoga Slim 7 14Q8X9"),
                compatible: Some("lenovo,yoga-slim7x"),
            },
            DeviceTree {
                chid: guid_str("d99f6cb2-4a96-5e4a-8e29-19d52dfc2870"),
                name: Some("LENOVO Yoga Slim 7 14Q8X9"),
                compatible: Some("lenovo,yoga-slim7x"),
            },
            DeviceTree {
                chid: guid_str("ee39b629-4187-5ff7-84c0-e354555562cd"),
                name: Some("LENOVO Yoga Slim 7 14Q8X9"),
                compatible: Some("lenovo,yoga-slim7x"),
            },
            DeviceTree {
                chid: guid_str("f7f92b85-ff01-5e93-a453-c7f91029aa55"),
                name: Some("LENOVO Yoga Slim 7 14Q8X9"),
                compatible: Some("lenovo,yoga-slim7x"),
            },
            DeviceTree {
                chid: guid_str("fdb12a4f-1e8b-524e-97b5-feef23a8a8da"),
                name: Some("LENOVO Yoga Slim 7 14Q8X9"),
                compatible: Some("lenovo,yoga-slim7x"),
            },
            DeviceTree {
                chid: guid_str("01bf1e61-d2e0-518b-bb46-eb4d1f2b1af1"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,denali"),
            },
            DeviceTree {
                chid: guid_str("06128fee-87dc-50f6-8a3f-97cd9a6d8bf6"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,denali"),
            },
            DeviceTree {
                chid: guid_str("14b96570-4bc4-541a-9aef-1b7e2b61d7cd"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,denali"),
            },
            DeviceTree {
                chid: guid_str("16a47337-1f8b-5bd3-b3bd-8e50b31cb1c9"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,denali"),
            },
            DeviceTree {
                chid: guid_str("48b86a5e-1955-5799-9577-150f9e1a69e4"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,denali"),
            },
            DeviceTree {
                chid: guid_str("584a5084-15f2-5d20-917b-57f299e61f7e"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,denali"),
            },
            DeviceTree {
                chid: guid_str("5a384f15-464d-5da8-9311-a2c021759afc"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,denali"),
            },
            DeviceTree {
                chid: guid_str("66f9d954-5c66-5577-b3e4-e3f14f87d2ff"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,denali"),
            },
            DeviceTree {
                chid: guid_str("7cef06f5-e7e6-56d7-b123-a6d640a5d302"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,denali"),
            },
            DeviceTree {
                chid: guid_str("84b2e1d1-e695-5f41-8c41-cf1f059c616a"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,denali"),
            },
            DeviceTree {
                chid: guid_str("95971fb3-d478-591f-9ea3-eb0af0d1dfb5"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,denali"),
            },
            DeviceTree {
                chid: guid_str("aca467c0-5fc2-59ad-8ed5-1b7a0988d11c"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,denali"),
            },
            DeviceTree {
                chid: guid_str("c9c14db9-2b61-597a-a4ba-84397fe75f63"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,denali"),
            },
            DeviceTree {
                chid: guid_str("11696377-327d-5ad1-b01d-02a7dbb9b99a"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus13"),
            },
            DeviceTree {
                chid: guid_str("224ba2ff-14c1-5b33-ac10-079ccc217be2"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus13"),
            },
            DeviceTree {
                chid: guid_str("3c329240-a447-5ec5-b79b-d1149420ac62"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus13"),
            },
            DeviceTree {
                chid: guid_str("4ecd5e53-42ea-51a3-9602-aecdfee5c09d"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus13"),
            },
            DeviceTree {
                chid: guid_str("53368ca9-12d5-5ee1-820b-ce979fa2cb0b"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus13"),
            },
            DeviceTree {
                chid: guid_str("786c71b6-f60e-51c7-9ddc-f2999b75a3c5"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus13"),
            },
            DeviceTree {
                chid: guid_str("892e90c9-31e3-5131-a217-a02632dba5e9"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus13"),
            },
            DeviceTree {
                chid: guid_str("924900a0-9be2-53ca-90d7-b0e38827f5c5"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus13"),
            },
            DeviceTree {
                chid: guid_str("95c06fde-19b0-55dc-9ca6-55403bae23f5"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus13"),
            },
            DeviceTree {
                chid: guid_str("c735618b-d526-5f71-9651-8d149340d620"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus13"),
            },
            DeviceTree {
                chid: guid_str("cb196e28-20bc-5e78-93f1-0ac41726bcf8"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus13"),
            },
            DeviceTree {
                chid: guid_str("f0d12ad9-f530-5b56-96d8-897dd704059e"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus13"),
            },
            DeviceTree {
                chid: guid_str("fdfca0f3-41b6-5872-a2ea-53539fd5160c"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus13"),
            },
            DeviceTree {
                chid: guid_str("109cd8d8-6086-50b6-9c2d-d0aca0f418da"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus15"),
            },
            DeviceTree {
                chid: guid_str("224ba2ff-14c1-5b33-ac10-079ccc217be2"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus15"),
            },
            DeviceTree {
                chid: guid_str("27ec66e4-3f81-5c06-997e-e1ea0a98b8a1"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus15"),
            },
            DeviceTree {
                chid: guid_str("3c329240-a447-5ec5-b79b-d1149420ac62"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus15"),
            },
            DeviceTree {
                chid: guid_str("892e90c9-31e3-5131-a217-a02632dba5e9"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus15"),
            },
            DeviceTree {
                chid: guid_str("90482ef5-831d-5069-8d40-92d339a75c77"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus15"),
            },
            DeviceTree {
                chid: guid_str("924900a0-9be2-53ca-90d7-b0e38827f5c5"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus15"),
            },
            DeviceTree {
                chid: guid_str("c735618b-d526-5f71-9651-8d149340d620"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus15"),
            },
            DeviceTree {
                chid: guid_str("e1fbd53f-3738-5fa6-aa7b-5ae319663d6b"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus15"),
            },
            DeviceTree {
                chid: guid_str("e4cef54f-d5b2-56b1-8aa0-07b48c3deedf"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus15"),
            },
            DeviceTree {
                chid: guid_str("e56cd9fa-d992-5947-9f80-82345827e8e6"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus15"),
            },
            DeviceTree {
                chid: guid_str("ebce3085-12c1-58f3-9456-ccdf741a1538"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus15"),
            },
            DeviceTree {
                chid: guid_str("f91b1a95-926c-5fd7-9826-4a101e142f97"),
                name: Some("Microsoft Corporation Surface"),
                compatible: Some("microsoft,romulus15"),
            },
            DeviceTree {
                chid: guid_str("00bc5418-646d-5bab-b772-4efb06f4e7f1"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407qa-lcd"),
            },
            DeviceTree {
                chid: guid_str("0b8b84da-462b-5620-bdb5-70272e0ddd94"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407qa-lcd"),
            },
            DeviceTree {
                chid: guid_str("14643426-35fa-5a20-bc31-3b6095d2b451"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407qa-lcd"),
            },
            DeviceTree {
                chid: guid_str("24652d54-00f4-59ae-96fb-f7adbfa4a939"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407qa-lcd"),
            },
            DeviceTree {
                chid: guid_str("3ca4e2d9-50df-51a5-a87f-4636d425e97d"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407qa-lcd"),
            },
            DeviceTree {
                chid: guid_str("59793319-4344-5755-9194-17f29f030d5d"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407qa-lcd"),
            },
            DeviceTree {
                chid: guid_str("7e6d3df4-bf5f-59ab-ad7b-e00677c0ae5a"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407qa-lcd"),
            },
            DeviceTree {
                chid: guid_str("85f50d27-f4cb-54df-9aae-f6f09700b132"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407qa-lcd"),
            },
            DeviceTree {
                chid: guid_str("8a72a2ea-3971-55e3-b982-cb6b82868f0e"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407qa-lcd"),
            },
            DeviceTree {
                chid: guid_str("a034eff6-2891-5de0-b0db-9c5ff350b968"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407qa-lcd"),
            },
            DeviceTree {
                chid: guid_str("a5b5becc-2a55-5017-b159-087f3846da26"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407qa-lcd"),
            },
            DeviceTree {
                chid: guid_str("a8425d85-573a-56f8-9d9d-98a196d712fa"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407qa-lcd"),
            },
            DeviceTree {
                chid: guid_str("c6d100b1-9de7-5636-a3cc-f28fa46fb926"),
                name: Some("ASUSTeK COMPUTER INC. ASUS Zenbook A14"),
                compatible: Some("asus,zenbook-a14-ux3407qa-lcd"),
            },
            DeviceTree {
                chid: guid_str("1d8361a7-1b3a-5915-8a35-03a3c1cf9c2e"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14-fe1"),
            },
            DeviceTree {
                chid: guid_str("271cca67-bd9e-5dd6-8e5f-b5f6b969da97"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14-fe1"),
            },
            DeviceTree {
                chid: guid_str("29a43fda-41e4-5db5-b6d3-012d0674d84f"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14-fe1"),
            },
            DeviceTree {
                chid: guid_str("38e7030f-993f-5bda-9ce8-ae13a13d7b5a"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14-fe1"),
            },
            DeviceTree {
                chid: guid_str("3fdb269e-c359-5004-b4e3-8541ef3580c9"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14-fe1"),
            },
            DeviceTree {
                chid: guid_str("5120f011-8f7e-5ca5-9143-de545e288712"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14-fe1"),
            },
            DeviceTree {
                chid: guid_str("612e268b-1233-5af6-b478-5596d3573d35"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14-fe1"),
            },
            DeviceTree {
                chid: guid_str("6fe7a469-b01a-5530-9a34-2dd089e0e006"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14-fe1"),
            },
            DeviceTree {
                chid: guid_str("9c3e4a5b-8fa2-5045-9c4f-441307fa3b08"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14-fe1"),
            },
            DeviceTree {
                chid: guid_str("d4db0558-de1b-562b-bc23-3e0caadd4c94"),
                name: Some("HP 103C_5335M8 HP OmniBook X"),
                compatible: Some("hp,omnibook-x14-fe1"),
            },
            DeviceTree {
                chid: guid_str("058c0739-1843-5a10-bab7-fae8aaf30add"),
                name: Some("Acer Swift 14 AI"),
                compatible: Some("acer,swift-sf14-11"),
            },
            DeviceTree {
                chid: guid_str("100917f4-9c0a-5ac3-a297-794222da9bc9"),
                name: Some("Acer Swift 14 AI"),
                compatible: Some("acer,swift-sf14-11"),
            },
            DeviceTree {
                chid: guid_str("20c2cf2f-231c-5d02-ae9b-c837ab5653ed"),
                name: Some("Acer Swift 14 AI"),
                compatible: Some("acer,swift-sf14-11"),
            },
            DeviceTree {
                chid: guid_str("27d2dba8-e6f1-5c19-ba1c-c25a4744c161"),
                name: Some("Acer Swift 14 AI"),
                compatible: Some("acer,swift-sf14-11"),
            },
            DeviceTree {
                chid: guid_str("331d7526-8b88-5923-bf98-450cf3ea82a4"),
                name: Some("Acer Swift 14 AI"),
                compatible: Some("acer,swift-sf14-11"),
            },
            DeviceTree {
                chid: guid_str("3f49141c-d8fb-5a6f-8b4a-074a2397874d"),
                name: Some("Acer Swift 14 AI"),
                compatible: Some("acer,swift-sf14-11"),
            },
            DeviceTree {
                chid: guid_str("676172cd-d185-53ed-aac6-245d0caa02c4"),
                name: Some("Acer Swift 14 AI"),
                compatible: Some("acer,swift-sf14-11"),
            },
            DeviceTree {
                chid: guid_str("6a12c9bc-bcfa-5448-9f66-4159dbe8c326"),
                name: Some("Acer Swift 14 AI"),
                compatible: Some("acer,swift-sf14-11"),
            },
            DeviceTree {
                chid: guid_str("7c107a7f-2d77-51aa-aef8-8d777e26ffbc"),
                name: Some("Acer Swift 14 AI"),
                compatible: Some("acer,swift-sf14-11"),
            },
            DeviceTree {
                chid: guid_str("98ad068a-f812-5f13-920c-3ff3d34d263f"),
                name: Some("Acer Swift 14 AI"),
                compatible: Some("acer,swift-sf14-11"),
            },
            DeviceTree {
                chid: guid_str("ee8fa049-e5f4-51e4-89d8-89a0140b8f38"),
                name: Some("Acer Swift 14 AI"),
                compatible: Some("acer,swift-sf14-11"),
            },
            DeviceTree {
                chid: guid_str("f2ea7095-999d-5e5b-8f2a-4b636a1e399f"),
                name: Some("Acer Swift 14 AI"),
                compatible: Some("acer,swift-sf14-11"),
            },
            DeviceTree {
                chid: guid_str("f55122fb-303f-58bc-b342-6ef653956d1d"),
                name: Some("Acer Swift 14 AI"),
                compatible: Some("acer,swift-sf14-11"),
            },
        ];
        assert_eq!(r, e);
    }

    #[test]
    fn test_serialize_single() {
        let mut actual = Vec::new();
        serialize_chid_mappings(
            &mut actual,
            &[ChidMapping::DeviceTree {
                chid: Guid::default(),
                name: Some("MyDevice"),
                compatible: Some("my,compatible"),
            }],
        )
        .unwrap();

        let mut expected = Vec::new();
        push_mapping(
            &mut expected,
            ChidMappingHeader {
                descriptor: 0x1000_001c,
                chid: Guid::default(),
            },
            ChidMappingDeviceTree {
                name_offset: 56,
                compatible_offset: 65,
            }
            .as_bytes(),
        ); // type 1, length 28
        push_mapping(&mut expected, ChidMappingHeader::default(), &[0u8; 8]); // terminator (long, ukify compatible)
        push_strings(&mut expected, &["MyDevice", "my,compatible"]);

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_serialize_multiple() {
        let mut actual = Vec::new();
        serialize_chid_mappings(
            &mut actual,
            &[
                ChidMapping::DeviceTree {
                    chid: Guid::default(),
                    name: Some("MyDevice"),
                    compatible: Some("my,compatible"),
                },
                ChidMapping::Unknown {
                    kind: 5,
                    chid: Guid::default(),
                    body: &[0x42u8; 180],
                },
                ChidMapping::UefiFw {
                    chid: Guid::read_from_bytes(&[1u8; 16]).unwrap(),
                    name: Some("MyFirmware"),
                    fwid: Some("FWID1234"),
                },
            ],
        )
        .unwrap();

        let mut expected = Vec::new();
        push_mapping(
            &mut expected,
            ChidMappingHeader {
                descriptor: 0x1000_001c,
                chid: Guid::default(),
            },
            ChidMappingDeviceTree {
                name_offset: 284,
                compatible_offset: 293,
            }
            .as_bytes(),
        ); // type 1, length 28
        push_mapping(
            &mut expected,
            ChidMappingHeader {
                descriptor: 0x5000_00c8,
                chid: Guid::default(),
            },
            &[0x42u8; 180],
        ); // unknown type 5, length 200, skipped
        push_mapping(
            &mut expected,
            ChidMappingHeader {
                descriptor: 0x2000_001c,
                chid: Guid::read_from_bytes(&[1u8; 16]).unwrap(),
            },
            ChidMappingUefiFw {
                name_offset: 307,
                fwid_offset: 318,
            }
            .as_bytes(),
        ); // type 2, length 28
        push_mapping(&mut expected, ChidMappingHeader::default(), &[0u8; 8]); // terminator (long, ukify compatible)
        push_strings(
            &mut expected,
            &["MyDevice", "my,compatible", "MyFirmware", "FWID1234"],
        );

        assert_eq!(actual, expected);
    }
}

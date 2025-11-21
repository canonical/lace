// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

use alloc::string::String;
use core::fmt::{self, Display, Formatter};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

const EDID_PATTERN: [u8; 8] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00];
const HEXCHARS_LOWER: &[u8; 16] = b"0123456789abcdef";

/// Header of an EDID
#[repr(C)]
#[derive(Clone, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
struct EdidHeader {
    pattern: [u8; 8],
    manufacturer_id: u16,           // big-endian
    manufacturer_product_code: u16, // little-endian
    serial_number: u32,             // little-endian
    week_of_manufacture: u8,
    year_of_manufacture: u8,
    edid_version: u8,
    edid_revision: u8,
}

/// Represents a parsed EDID structure
#[derive(Debug, Clone)]
pub struct ParsedEdid {
    header: EdidHeader,
}

#[derive(Debug, Clone, Copy)]
pub enum EdidParseError {
    InvalidSize,
    InvalidHeader,
    InvalidPanelId,
}

impl Display for EdidParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            EdidParseError::InvalidSize => write!(f, "EDID data is too small"),
            EdidParseError::InvalidHeader => write!(f, "EDID header is invalid"),
            EdidParseError::InvalidPanelId => write!(f, "Panel ID could not be found"),
        }
    }
}

impl ParsedEdid {
    pub fn parse(edid_data: &[u8]) -> Result<Self, EdidParseError> {
        // Spec says this is the minimum size
        if edid_data.len() < 128 {
            return Err(EdidParseError::InvalidSize);
        }

        // Parse header and validate magic pattern
        let Ok((mut header, _)) = EdidHeader::read_from_prefix(edid_data) else {
            return Err(EdidParseError::InvalidSize);
        };
        if header.pattern != EDID_PATTERN {
            return Err(EdidParseError::InvalidHeader);
        }

        // Convert fields to host endianness
        header.manufacturer_id = u16::from_be(header.manufacturer_id);
        header.manufacturer_product_code = u16::from_le(header.manufacturer_product_code);
        header.serial_number = u32::from_le(header.serial_number);

        // Return parsed EDID
        Ok(ParsedEdid { header })
    }

    pub fn panel_id(&self) -> Result<String, EdidParseError> {
        fn mfr_letter(val: u16) -> Result<u8, EdidParseError> {
            if (1..=26).contains(&val) {
                Ok(b'A' + (val as u8 - 1))
            } else {
                Err(EdidParseError::InvalidPanelId)
            }
        }

        let mut s = String::new();
        s.push(mfr_letter((self.header.manufacturer_id >> 10) & 0x1F)? as char);
        s.push(mfr_letter((self.header.manufacturer_id >> 5) & 0x1F)? as char);
        s.push(mfr_letter(self.header.manufacturer_id & 0x1F)? as char);
        s.push(
            HEXCHARS_LOWER[((self.header.manufacturer_product_code >> 12) & 0xF) as usize] as char,
        );
        s.push(
            HEXCHARS_LOWER[((self.header.manufacturer_product_code >> 8) & 0xF) as usize] as char,
        );
        s.push(
            HEXCHARS_LOWER[((self.header.manufacturer_product_code >> 4) & 0xF) as usize] as char,
        );
        s.push(HEXCHARS_LOWER[(self.header.manufacturer_product_code & 0xF) as usize] as char);
        Ok(s)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_valid_edid() {
        let mut edid_data: [u8; 128] = [0; 128];
        edid_data[0..20].copy_from_slice(&[
            0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, // pattern
            0x30, 0x64, // mfr id
            0xab, 0x89, // product code
            0x78, 0x56, 0x34, 0x12, // serial number
            0x1a, 0x1b, // week and year
            0x01, 0x03, // version
        ]);
        let parsed = ParsedEdid::parse(&edid_data).expect("Failed to parse valid EDID");
        let panel_id = parsed.panel_id().expect("Failed to get panel ID");
        assert_eq!(panel_id, "LCD89ab");
        assert_eq!(parsed.header.serial_number, 0x12345678);
        assert_eq!(parsed.header.week_of_manufacture, 0x1a);
        assert_eq!(parsed.header.year_of_manufacture, 0x1b);
        assert_eq!(parsed.header.edid_version, 0x01);
        assert_eq!(parsed.header.edid_revision, 0x03);
    }

    #[test]
    fn test_parse_invalid_size() {
        let edid_data: [u8; 100] = [0; 100];
        let result = ParsedEdid::parse(&edid_data);
        assert!(matches!(result, Err(EdidParseError::InvalidSize)));
    }

    #[test]
    fn test_parse_invalid_header() {
        let edid_data: [u8; 128] = [0; 128];
        let result = ParsedEdid::parse(&edid_data);
        assert!(matches!(result, Err(EdidParseError::InvalidHeader)));
    }
}

// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

use core::marker::PhantomData;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct SmbiosEntryPoint {
    pub anchor_string: [u8; 4],
    pub entry_point_structure_checksum: u8,
    pub entry_point_length: u8,
    pub major_version: u8,
    pub minor_version: u8,
    pub max_structure_size: u16,
    pub entry_point_revision: u8,
    pub formatted_area: [u8; 5],
    pub intermediate_anchor_string: [u8; 5],
    pub intermediate_checksum: u8,
    pub table_length: u16,
    pub table_address: u32,
    pub number_of_smbios_structures: u16,
    pub smbios_bcd_revision: u8,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct Smbios3EntryPoint {
    pub anchor_string: [u8; 5],
    pub entry_point_structure_checksum: u8,
    pub entry_point_length: u8,
    pub major_version: u8,
    pub minor_version: u8,
    pub docrev: u8,
    pub entry_point_revision: u8,
    pub reserved: u8,
    pub table_maximum_size: u32,
    pub table_address: u64,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct SmbiosHeader {
    pub type_: u8,
    pub length: u8,
    pub handle: [u8; 2],
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct SmbiosTableType0 {
    pub header: SmbiosHeader,
    pub vendor: u8,
    pub bios_version: u8,
    pub bios_segment: u16,
    pub bios_release_date: u8,
    pub bios_size: u8,
    pub bios_characteristics: u64,
    // Versions 2.1 to 2.3 might have 1 or 2 extension bytes
    // here, but we don't care about those.
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct SmbiosTableType0_24 {
    pub header: SmbiosHeader,
    pub vendor: u8,
    pub bios_version: u8,
    pub bios_segment: u16,
    pub bios_release_date: u8,
    pub bios_size: u8,
    pub bios_characteristics: u64,
    // Version 2.4 is defined to have exactly 2 extension bytes
    // and the exact list of fields below.
    pub bios_characteristics_ext: [u8; 2],
    pub bios_major_release: u8,
    pub bios_minor_release: u8,
    pub ec_fw_major_release: u8,
    pub ec_fw_minor_release: u8,
    pub ext_bios_rom_size: u16,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct SmbiosTableType1 {
    pub header: SmbiosHeader,
    pub manufacturer: u8,
    pub product_name: u8,
    pub version: u8,
    pub serial_number: u8,
    pub uuid: crate::Guid,
    pub wake_up_type: u8,
    pub sku_number: u8,
    pub family: u8,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct SmbiosTableType2 {
    pub header: SmbiosHeader,
    pub manufacturer: u8,
    pub product_name: u8,
    pub version: u8,
    pub serial_number: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmbiosError {
    TableNotFound,
    InvalidTable,
}

pub struct SmbiosTable<'s, T: FromBytes + KnownLayout + Immutable> {
    table_type: PhantomData<T>,
    table: &'s [u8],
    strings: &'s [u8],
}

impl<'s, T: FromBytes + KnownLayout + Immutable> SmbiosTable<'s, T> {
    pub fn table(&self) -> &'s T {
        let (t, _) = T::ref_from_prefix(self.table).unwrap();
        t
    }

    pub fn get_string(&self, i: usize) -> Option<&'s [u8]> {
        if i < 1 {
            // String index is 1 based
            return None;
        }
        self.strings.split(|b| *b == 0).nth(i - 1)
    }
}

pub fn find_smbios_table_by_type<'s, T: FromBytes + KnownLayout + Immutable>(
    mut s: &'s [u8],
    type_: u8,
) -> Result<SmbiosTable<'s, T>, SmbiosError> {
    loop {
        // Get table header
        let Ok((header, _)) = SmbiosHeader::ref_from_prefix(s) else {
            return Err(SmbiosError::InvalidTable);
        };

        // Check if the size in the header covers the header
        if (header.length as usize) < core::mem::size_of::<SmbiosHeader>() {
            return Err(SmbiosError::InvalidTable);
        }

        // Check if we really have as much data as specified
        let Some((table, rest)) = s.split_at_checked(header.length as usize) else {
            return Err(SmbiosError::InvalidTable);
        };

        // Find the end of the strings section
        let Some(end_of_strings) = crate::find_byte_sequence(rest, b"\0\0") else {
            return Err(SmbiosError::InvalidTable);
        };

        match header.type_ {
            127 => {
                // End-of-tables indicator
                return Err(SmbiosError::TableNotFound);
            }
            t if t == type_ => {
                // Matching type
                // Check if the size in the header covers the table
                if (header.length as usize) < core::mem::size_of::<T>() {
                    return Err(SmbiosError::InvalidTable);
                }
                return Ok(SmbiosTable {
                    table_type: PhantomData,
                    table,
                    strings: &rest[..end_of_strings],
                });
            }
            _ => {
                // Move to next table
                s = &rest[end_of_strings + 2..];
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use core::mem::size_of;

    // Helper to append a table with the given type and payload length (header is 4 bytes)
    fn push_table(buf: &mut Vec<u8>, t: u8, len: usize, strings: &[u8]) {
        buf.push(t); // type
        buf.push(len as u8); // length
        buf.extend_from_slice(&[0x34, 0x12]); // handle (arbitrary)
        buf.resize(buf.len() - 4 + len, 0); // payload zeroed
        buf.extend_from_slice(strings); // strings including terminating \0\0
    }

    #[test]
    fn test_find_smbios_table_by_type() {
        // Build a small SMBIOS stream: Type 0 table, then Type 1 table, then End (127)
        let mut buf: Vec<u8> = Vec::new();

        // Type 0 with two strings: "VENDOR", "VER"
        push_table(
            &mut buf,
            0,
            size_of::<SmbiosTableType0>(),
            b"VENDOR\0VER\0\0",
        );
        // Type 1 with two strings: "MFG", "PROD"
        push_table(&mut buf, 1, size_of::<SmbiosTableType1>(), b"MFG\0PROD\0\0");
        // End-of-table 127
        push_table(&mut buf, 127, size_of::<SmbiosHeader>(), b"\0\0");

        // Should find Type 1 successfully
        let tbl: SmbiosTable<'_, SmbiosTableType1> =
            find_smbios_table_by_type(&buf, 1).expect("type 1 should be found");

        // Header type should be 1 and length should match struct size
        assert_eq!(tbl.table().header.type_, 1);
        assert_eq!(
            tbl.table().header.length as usize,
            size_of::<SmbiosTableType1>()
        );

        // First string (index 2 in our accessor) should be "MFG"
        assert_eq!(tbl.get_string(1), Some(&b"MFG"[..]));
        // Second string should be "PROD"
        assert_eq!(tbl.get_string(2), Some(&b"PROD"[..]));
    }

    #[test]
    fn test_type_not_found_returns_table_not_found() {
        let mut buf: Vec<u8> = Vec::new();

        // Only a Type 0 then End (127)
        push_table(&mut buf, 0, size_of::<SmbiosTableType0>(), b"VENDOR\0\0");
        // End-of-table 127
        push_table(&mut buf, 127, size_of::<SmbiosHeader>(), b"\0\0");

        let res: Result<SmbiosTable<'_, SmbiosTableType1>, SmbiosError> =
            find_smbios_table_by_type(&buf, 1);
        assert!(matches!(res, Err(SmbiosError::TableNotFound)));
    }

    #[test]
    fn test_invalid_when_header_length_exceeds_buffer() {
        // Malformed: claims big length but buffer ends
        let buf = vec![
            1, 64, // type 1, length 64 (too big for following data)
            0x00, 0x00, // handle
                  // no payload, no strings
        ];
        let res: Result<SmbiosTable<'_, SmbiosTableType1>, SmbiosError> =
            find_smbios_table_by_type(&buf, 1);
        assert!(matches!(res, Err(SmbiosError::InvalidTable)));
    }

    #[test]
    fn test_invalid_when_missing_double_zero_string_terminator() {
        // Well-sized header and payload but strings not properly terminated with \0\0
        let mut buf: Vec<u8> = Vec::new();
        push_table(&mut buf, 1, size_of::<SmbiosTableType1>(), b"ONLYONE\0");

        let res: Result<SmbiosTable<'_, SmbiosTableType1>, SmbiosError> =
            find_smbios_table_by_type(&buf, 1);
        assert!(matches!(res, Err(SmbiosError::InvalidTable)));
    }
}

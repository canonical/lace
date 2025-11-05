// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

use zerocopy::{FromBytes, Immutable, KnownLayout};

#[repr(C, packed)]
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
#[derive(FromBytes,Immutable,KnownLayout)]
pub struct SmbiosHeader {
    pub type_: u8,
    pub length: u8,
    pub handle: [u8; 2],
}

#[repr(C, packed)]
#[derive(FromBytes,Immutable,KnownLayout)]
pub struct SmbiosTableType0 {
    pub header: SmbiosHeader,
    pub vendor: u8,
    pub bios_version: u8,
    pub bios_segment: u16,
    pub bios_release_date: u8,
    pub bios_size: u8,
    pub bios_characteristics: u64,
    pub bios_characteristics_ext: [u8; 2],
}

#[repr(C, packed)]
#[derive(FromBytes,Immutable,KnownLayout)]
pub struct SmbiosTableType1 {
    pub header: SmbiosHeader,
    pub manufacturer: u8,
    pub product_name: u8,
    pub version: u8,
    pub serial_number: u8,
    pub uuid: EFI_GUID,
    pub wake_up_type: u8,
    pub sku_number: u8,
    pub family: u8,
}


#[repr(C, packed)]
#[derive(FromBytes,Immutable,KnownLayout)]
pub struct EFI_GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

#[repr(C, packed)]
#[derive(FromBytes,Immutable,KnownLayout)]
pub struct SmbiosTableType2 {
    pub header: SmbiosHeader,
    pub manufacturer: u8,
    pub product_name: u8,
    pub version: u8,
    pub serial_number: u8,
}

pub enum SmbiosError {
    TableNotFound,
    InvalidTable,
}

pub struct SmbiosTable<'s> {
    table: &'s [u8],
    strings: &'s [u8],
}

pub fn find_smbios_table_by_type<'s, T: FromBytes>(mut s: &'s [u8], type_: u8) -> Result<SmbiosTable<'s>, SmbiosError>  {
    loop {
        // Get table header
        let Ok((header, _)) = SmbiosHeader::ref_from_prefix(s) else {
            return Err(SmbiosError::InvalidTable)
        };

        // Check if the size in the header makes sense
        if (header.length as usize) < core::mem::size_of::<SmbiosHeader>() ||
            (header.length as usize) < core::mem::size_of::<T>() {
            return Err(SmbiosError::InvalidTable)
        }

        // Check if we really have as much data as specified
        let Some((table, rest)) = s.split_at_checked(header.length as usize) else {
            return Err(SmbiosError::InvalidTable)
        };

        match header.type_ {
            // End-of-tables indicator
            127 => {
                return Err(SmbiosError::TableNotFound)
            }
            // Matching type
            t if t == type_ => {
                return Ok(SmbiosTable { table, rest })
            }
            _ => ()
        }

        let mut i = 0;
        while i + 1 < rest.len() && (rest[i] != 0 || rest[i+1] != 0) {
            i += 1;
        }
        if i + 1 >= rest.len() {
            return Err(SmbiosError::TableNotFound)
        }

        s = &rest[2..];
    }
}

/*

#[cfg(test)]
mod test {
    pub use super::*;

    #[test]
    fn test_find_smbios_table_by_type() {
        let data: &[u8] = &[
            0x00, 0x05, 0x01, 0x02, 0x03, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x01, 0x04, 0x05, 0x06, 0x07, 0x08, 0x00, 0x00, 0x00,
            0x02, 0x03, 0x09, 0x0A, 0x0B,
            0x00, 0x00,
        ];

        let table = find_smbios_table_by_type(data, 1).unwrap();
        assert_eq!(table, &data[11..20]);

        let table = find_smbios_table_by_type(data, 2).unwrap();
        assert_eq!(table, &data[20..25]);

        let table = find_smbios_table_by_type(data, 3);
        assert!(table.is_none());
    }   
}

*/

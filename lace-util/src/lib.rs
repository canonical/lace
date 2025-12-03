// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod chid;
pub mod chid_mapping;
pub mod edid;
pub mod peimage;
pub mod sha1;
pub mod smbios;

use core::fmt::{self, Debug, Display, Write};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

pub fn find_byte_sequence(s: &[u8], sub: &[u8]) -> Option<usize> {
    if s.len() < sub.len() {
        return None;
    }
    (0..s.len() - sub.len() + 1).find(|&i| &s[i..i + sub.len()] == sub)
}

pub fn hexdump<W: Write>(mut w: W, s: &[u8]) -> fmt::Result {
    for (i, b) in s.iter().enumerate() {
        if i % 16 == 0 {
            write!(w, "{:04x}  ", i)?;
        }
        write!(w, "{:02x}", b)?;
        if (i + 1) % 16 == 0 || i + 1 == s.len() {
            writeln!(w)?;
        } else if (i + 1) % 8 == 0 {
            write!(w, "  ")?;
        } else {
            write!(w, " ")?;
        }
    }
    Ok(())
}

#[macro_export]
macro_rules! align_up {
    ($val:expr, $bound:expr $(,)?) => {{
        let _bound = $bound;
        $val.div_ceil(_bound) * _bound
    }};
}

#[macro_export]
macro_rules! align_down {
    ($val:expr, $bound:expr $(,)?) => {{
        let _bound = $bound;
        $val / _bound * _bound
    }};
}

#[macro_export]
macro_rules! count_blocks_aligned_up {
    ($val:expr, $block_size:expr $(,)?) => {
        $val.div_ceil($block_size)
    };
}

#[macro_export]
macro_rules! count_blocks_aligned_down {
    ($val:expr, $block_size:expr $(,)?) => {
        $val / $block_size
    };
}

#[repr(C)]
#[derive(
    Clone, Copy, Default, PartialEq, Eq, Hash, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl Debug for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "guid_str(\"{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}\")",
            self.data1,
            self.data2,
            self.data3,
            self.data4[0],
            self.data4[1],
            self.data4[2],
            self.data4[3],
            self.data4[4],
            self.data4[5],
            self.data4[6],
            self.data4[7]
        )?;
        Ok(())
    }
}

impl Display for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            self.data1,
            self.data2,
            self.data3,
            self.data4[0],
            self.data4[1],
            self.data4[2],
            self.data4[3],
            self.data4[4],
            self.data4[5],
            self.data4[6],
            self.data4[7]
        )?;
        Ok(())
    }
}

pub const fn guid_str(s: &str) -> Guid {
    // Expects a string in the format "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
    // Accepts both lowercase and uppercase hex digits and dashes at the correct positions.
    // Panics if the format is invalid.
    let bytes = s.as_bytes();
    assert!(bytes.len() == 36, "GUID string must be 36 characters long");
    assert!(
        bytes[8] == b'-' && bytes[13] == b'-' && bytes[18] == b'-' && bytes[23] == b'-',
        "GUID dashes at wrong positions"
    );

    const fn hex(b: u8) -> u8 {
        match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => panic!("Invalid hex digit in GUID"),
        }
    }

    const fn byte(a: u8, b: u8) -> u8 {
        (hex(a) << 4) | hex(b)
    }

    Guid {
        data1: ((hex(bytes[0]) as u32) << 28)
            | ((hex(bytes[1]) as u32) << 24)
            | ((hex(bytes[2]) as u32) << 20)
            | ((hex(bytes[3]) as u32) << 16)
            | ((hex(bytes[4]) as u32) << 12)
            | ((hex(bytes[5]) as u32) << 8)
            | ((hex(bytes[6]) as u32) << 4)
            | (hex(bytes[7]) as u32),
        data2: ((hex(bytes[9]) as u16) << 12)
            | ((hex(bytes[10]) as u16) << 8)
            | ((hex(bytes[11]) as u16) << 4)
            | (hex(bytes[12]) as u16),
        data3: ((hex(bytes[14]) as u16) << 12)
            | ((hex(bytes[15]) as u16) << 8)
            | ((hex(bytes[16]) as u16) << 4)
            | (hex(bytes[17]) as u16),
        data4: [
            byte(bytes[19], bytes[20]),
            byte(bytes[21], bytes[22]),
            byte(bytes[24], bytes[25]),
            byte(bytes[26], bytes[27]),
            byte(bytes[28], bytes[29]),
            byte(bytes[30], bytes[31]),
            byte(bytes[32], bytes[33]),
            byte(bytes[34], bytes[35]),
        ],
    }
}

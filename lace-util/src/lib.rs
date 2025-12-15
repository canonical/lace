// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod chid;
pub mod chid_mapping;
pub mod chid_matcher;
pub mod edid;
pub mod peimage;
pub mod sha1;
pub mod smbios;

use core::cmp::Ordering;
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

#[derive(Debug, Clone, Copy, Default)]
pub struct GuidParseError;

impl Display for GuidParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid GUID format")
    }
}

impl Guid {
    /// Parse a GUID string in the format "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
    /// Accepts both lowercase and uppercase hex digits.
    /// Returns `None` if the string is not a valid GUID.
    pub const fn try_from_str(s: &str) -> Result<Self, GuidParseError> {
        let bytes = s.as_bytes();

        if bytes.len() != 36 {
            return Err(GuidParseError); // String must be 36 characters long
        }
        if bytes[8] != b'-' || bytes[13] != b'-' || bytes[18] != b'-' || bytes[23] != b'-' {
            return Err(GuidParseError); // Dashes at the wrong positions
        }

        match (
            hex_seq(bytes, 0, 8),
            hex_seq(bytes, 9, 13),
            hex_seq(bytes, 14, 18),
            hex_seq(bytes, 19, 21),
            hex_seq(bytes, 21, 23),
            hex_seq(bytes, 24, 26),
            hex_seq(bytes, 26, 28),
            hex_seq(bytes, 28, 30),
            hex_seq(bytes, 30, 32),
            hex_seq(bytes, 32, 34),
            hex_seq(bytes, 34, 36),
        ) {
            (
                Some(data1),
                Some(data2),
                Some(data3),
                Some(d4_0),
                Some(d4_1),
                Some(d4_2),
                Some(d4_3),
                Some(d4_4),
                Some(d4_5),
                Some(d4_6),
                Some(d4_7),
            ) => Ok(Guid {
                data1: data1 as u32,
                data2: data2 as u16,
                data3: data3 as u16,
                data4: [
                    d4_0 as u8, d4_1 as u8, d4_2 as u8, d4_3 as u8, d4_4 as u8, d4_5 as u8,
                    d4_6 as u8, d4_7 as u8,
                ],
            }),
            _ => Err(GuidParseError),
        }
    }
}

impl TryFrom<&str> for Guid {
    type Error = GuidParseError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::try_from_str(s)
    }
}

/// Parse a sequence of hexadecimal digits from `bytes[begin..end]` into a `u64`.
/// Returns `None` if the range is invalid or contains non-hex characters.
const fn hex_seq(bytes: &[u8], begin: usize, end: usize) -> Option<u64> {
    if begin >= end || end > bytes.len() {
        return None;
    }
    let mut value = 0u64;
    let mut i = begin;
    while i < end {
        let digit = match bytes[i] {
            b'0'..=b'9' => bytes[i] - b'0',
            b'a'..=b'f' => bytes[i] - b'a' + 10,
            b'A'..=b'F' => bytes[i] - b'A' + 10,
            _ => return None,
        };
        value = (value << 4) | digit as u64;
        i += 1;
    }
    Some(value)
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

/// Convert a GUID literal to a `Guid` at compile time.
/// Panics if the string is not a valid GUID.
pub const fn guid_str(s: &str) -> Guid {
    match Guid::try_from_str(s) {
        Ok(guid) => guid,
        Err(_) => panic!("invalid GUID literal"),
    }
}

/// Compare two version numbers in "major.minor" format.
pub fn cmp_maj_min(maj_a: u8, min_a: u8, maj_b: u8, min_b: u8) -> Ordering {
    match maj_a.cmp(&maj_b) {
        Ordering::Equal => min_a.cmp(&min_b),
        ord => ord,
    }
}

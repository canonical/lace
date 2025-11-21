// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod edid;
pub mod peimage;
pub mod sha1;
pub mod smbios;

use core::fmt::{self, Write};

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
    ($val:expr, $bound:expr $(,)?) => {
        $val.div_ceil($bound) * $bound
    };
}

#[macro_export]
macro_rules! align_down {
    ($val:expr, $bound:expr $(,)?) => {
        $val / $bound * $bound
    };
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

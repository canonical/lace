// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

#![cfg_attr(not(test), no_std)]

pub mod peimage;
pub mod sha1;
pub mod smbios;

use core::fmt::{self, Write};

pub fn find_byte_sequence(s: &[u8], sub: &[u8]) -> Option<usize> {
    if s.len() < sub.len() {
        return None;
    }
    for i in 0..s.len() - sub.len() + 1 {
        if &s[i..i + sub.len()] == sub {
            return Some(i);
        }
    }
    None
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
        ($val + $bound - 1) / $bound * $bound
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
        ($val + $block_size - 1) / $block_size
    };
}

#[macro_export]
macro_rules! count_blocks_aligned_down {
    ($val:expr, $block_size:expr $(,)?) => {
        $val / $block_size
    };
}

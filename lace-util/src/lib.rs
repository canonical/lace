// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

pub mod smbios;

fn find_byte_sequence(s: &[u8], sub: &[u8]) -> Option<usize> {
    if s.len() < sub.len() {
        return None
    }
    for i in 0..s.len()-sub.len()+1 {
        if &s[i..i+sub.len()] == sub {
            return Some(i);
        }
    }
    None
}

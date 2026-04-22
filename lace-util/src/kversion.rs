// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! Parse and order Debian-style kernel version suffixes of the shape
//! `X.Y.Z-REV-FLAVOUR` (e.g. `6.8.0-110-generic`).
//!
//! The first four numeric components compare numerically; the flavour
//! compares by a static ranking defined here. Suffixes that cannot be
//! fully parsed sort *last* so they don't shadow real kernels.

use alloc::string::{String, ToString};
use core::cmp::Ordering;

/// Flavour names in *descending* preference order. First = most
/// preferred. Names not in this list sort after all known flavours,
/// among themselves by lexicographic order.
static FLAVOUR_RANK: &[&str] = &["generic", "generic-hwe", "lowlatency", "lowlatency-hwe"];

/// Parsed `X.Y.Z-REV-FLAVOUR` kernel version. Holds the original
/// suffix so callers can build file paths back out of it.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct KernelVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub rev: u32,
    pub flavour: String,
    /// Original unparsed suffix, e.g. `"6.8.0-110-generic"`.
    pub raw: String,
}

impl KernelVersion {
    /// Parse a suffix. Returns `None` if any of the four numeric
    /// components is missing or non-numeric.
    pub fn parse(suffix: &str) -> Option<Self> {
        let mut parts = suffix.splitn(3, '-');
        let xyz = parts.next()?;
        let rev = parts.next()?;
        let flavour = parts.next().unwrap_or("");

        let mut xyz_parts = xyz.split('.');
        let major: u32 = xyz_parts.next()?.parse().ok()?;
        let minor: u32 = xyz_parts.next()?.parse().ok()?;
        let patch: u32 = xyz_parts.next()?.parse().ok()?;
        if xyz_parts.next().is_some() {
            return None;
        }
        let rev: u32 = rev.parse().ok()?;

        Some(Self {
            major,
            minor,
            patch,
            rev,
            flavour: flavour.to_string(),
            raw: suffix.to_string(),
        })
    }
}

fn flavour_rank(f: &str) -> Option<usize> {
    FLAVOUR_RANK.iter().position(|&known| known == f)
}

impl Ord for KernelVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher numeric = greater.
        (self.major, self.minor, self.patch, self.rev)
            .cmp(&(other.major, other.minor, other.patch, other.rev))
            .then_with(|| match (flavour_rank(&self.flavour), flavour_rank(&other.flavour)) {
                // Lower index = higher preference, invert so "more
                // preferred" sorts greater.
                (Some(a), Some(b)) => b.cmp(&a),
                (Some(_), None) => Ordering::Greater,
                (None, Some(_)) => Ordering::Less,
                (None, None) => self.flavour.cmp(&other.flavour),
            })
    }
}

impl PartialOrd for KernelVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_standard_suffix() {
        let v = KernelVersion::parse("6.8.0-110-generic").unwrap();
        assert_eq!(v.major, 6);
        assert_eq!(v.minor, 8);
        assert_eq!(v.patch, 0);
        assert_eq!(v.rev, 110);
        assert_eq!(v.flavour, "generic");
    }

    #[test]
    fn parse_hyphenated_flavour() {
        let v = KernelVersion::parse("6.8.0-110-generic-hwe").unwrap();
        assert_eq!(v.flavour, "generic-hwe");
    }

    #[test]
    fn reject_non_numeric() {
        assert!(KernelVersion::parse("abc").is_none());
        assert!(KernelVersion::parse("6.8-110-generic").is_none());
    }

    #[test]
    fn newer_rev_greater() {
        let a = KernelVersion::parse("6.8.0-110-generic").unwrap();
        let b = KernelVersion::parse("6.8.0-107-generic").unwrap();
        assert!(a > b);
    }

    #[test]
    fn higher_major_beats_rev() {
        let a = KernelVersion::parse("6.10.0-1-generic").unwrap();
        let b = KernelVersion::parse("6.8.0-999-generic").unwrap();
        assert!(a > b);
    }

    #[test]
    fn flavour_order_generic_beats_lowlatency() {
        let a = KernelVersion::parse("6.8.0-110-generic").unwrap();
        let b = KernelVersion::parse("6.8.0-110-lowlatency").unwrap();
        assert!(a > b);
    }

    #[test]
    fn known_flavour_beats_unknown() {
        let a = KernelVersion::parse("6.8.0-110-generic").unwrap();
        let b = KernelVersion::parse("6.8.0-110-zzz").unwrap();
        assert!(a > b);
    }
}

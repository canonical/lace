// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! CBFS (coreboot filesystem) structures, reader, and writer
//!
//! Provides the on-flash data structures, a reader for memory-mapped
//! flash images, and a writer for constructing or updating CBFS images.

mod r#priv;
mod reader;
mod writer;

pub use reader::{CbfsFile, CbfsFiles, CbfsReader};
pub use writer::CbfsWriter;

use lace_util_derive::Display;
use zerocopy::byteorder::{BE, U32};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

// Public constants for CBFS format

/// CBFS header magic: 'ORBC' in big-endian.
pub const CBFS_HEADER_MAGIC: u32 = 0x4F524243;
/// CBFS header version 2: '1112' in big-endian.
pub const CBFS_HEADER_VERSION2: u32 = 0x31313132;
/// CBFS file magic: "LARCHIVE".
pub const CBFS_FILE_MAGIC: [u8; 8] = *b"LARCHIVE";
/// CBFS default alignment.
pub const CBFS_DEFAULT_ALIGNMENT: u32 = 64;
/// CBFS architecture: x86.
pub const CBFS_ARCHITECTURE_X86: u32 = 0x00000001;

// File types
pub const CBFS_TYPE_DELETED: u32 = 0x00000000;
pub const CBFS_TYPE_NULL: u32 = 0xFFFFFFFF;
pub const CBFS_TYPE_BOOTBLOCK: u32 = 0x01;
pub const CBFS_TYPE_RAW: u32 = 0x50;

/// Errors returned by CBFS operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Display)]
pub enum CbfsError {
    /// ROM size must be a power of two between 4 and 2 GiB.
    #[display("ROM size must be a power of two between 4 and 2 GiB")]
    InvalidRomSize,
    /// ROM base address must be a multiple of ROM size.
    #[display("ROM base address must be a multiple of ROM size")]
    InvalidRomBase,
    /// Header pointer at end of ROM does not resolve to a valid offset.
    #[display("header pointer at end of ROM does not resolve to a valid offset")]
    InvalidHeaderPointer,
    /// Master header has invalid magic, version, size, alignment, or offset.
    #[display("master header has invalid magic, version, size, alignment, or offset")]
    InvalidHeader,
    /// Alignment must be a power of two.
    #[display("alignment must be a power of two")]
    InvalidAlignment,
    /// Entry has valid magic but invalid offset, length, or name.
    ///
    /// When returned by the walker, iteration is terminated: subsequent
    /// calls will return `None`.
    #[display("entry has valid magic but invalid offset, length, or name")]
    CorruptEntry,
    /// File name contains a NUL byte.
    #[display("file name contains a NUL byte")]
    NulInName,
    /// Data or name length exceeds `u32::MAX`.
    #[display("data or name length exceeds u32::MAX")]
    TooLarge,
    /// No NULL entry large enough for the requested file.
    #[display("no NULL entry large enough for the requested file")]
    NoSpace,
}

/// CBFS master header (on-flash format, all big-endian).
#[repr(C)]
#[derive(Clone, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct CbfsHeader {
    pub magic: U32<BE>,
    pub version: U32<BE>,
    pub romsize: U32<BE>,
    pub bootblocksize: U32<BE>,
    pub align: U32<BE>,
    pub offset: U32<BE>,
    pub architecture: U32<BE>,
    pub _pad: U32<BE>,
}

/// CBFS file entry header (on-flash format, all big-endian).
#[repr(C)]
#[derive(Clone, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct CbfsFileHeader {
    pub magic: [u8; 8],
    pub len: U32<BE>,
    pub r#type: U32<BE>,
    pub attributes_offset: U32<BE>,
    pub offset: U32<BE>,
}

// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.

use clap::Parser;
use std::ffi::OsString;

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Tool to collect CHIDs from a running system",
    long_about = "collect-hwids reads system hardware information (SMBIOS tables and EDID data) \
                  and generates Computer Hardware IDs (CHIDs).\n\n\
                  The tool outputs CHID information to stdout and optionally creates a ZIP archive \
                  containing the raw hardware data for use in firmware update metadata or debugging."
)]
pub struct Args {
    /// Output path for HWIDs ZIP archive
    ///
    /// Creates a ZIP archive containing SMBIOS tables, EDID data, and computed
    /// CHIDs. The archive can be used for firmware update metadata generation
    /// or for debugging hardware identification issues. If not specified, only
    /// CHID information is written to stdout.
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<OsString>,
}

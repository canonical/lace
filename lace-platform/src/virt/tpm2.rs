// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Virt platform TPM2 stub

use crate::Error;
use crate::tpm2::*;

/// No TPM is exposed by QEMU virt yet; treat measurement as a no-op.
pub fn hash_log_extend_event(
    _pcr_index: u32,
    _flags: ExtendFlags,
    _event_type: u32,
    _event_data: &[u8],
    _data_to_hash: &[u8],
) -> Result<(), Error> {
    Ok(())
}

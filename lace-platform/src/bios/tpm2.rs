// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! BIOS TPM2 abstraction.

use crate::Error;
pub use crate::iface::tpm2::*;

/// This is a no-op, we probably don't really want to support TPM on BIOS.
/// But if we end up having a native driver it might be worth wiring up the interface.
pub fn hash_log_extend_event(
    _pcr_index: u32,
    _flags: ExtendFlags,
    _event_type: u32,
    _event_data: &[u8],
    _data_to_hash: &[u8],
) -> Result<(), Error> {
    Ok(())
}

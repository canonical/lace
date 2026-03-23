// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! UEFI TPM2 abstraction.

pub use crate::iface::tpm2::*;
use crate::{Error, debugln};
use uefi::proto::tcg::v2 as efi_tcg_v2;

/// Hashes the given data, logs the event and extends the PCR with the hash.
/// The event will only be logged if the "EXTEND_ONLY" flag is not set.
pub fn hash_log_extend_event(
    pcr_index: u32,
    flags: ExtendFlags,
    event_type: u32,
    event_data: &[u8],
    data_to_hash: &[u8],
) -> Result<(), Error> {
    if let Ok(mut proto) = super::open_protocol_exclusive::<efi_tcg_v2::Tcg>() {
        // Prepare buffer for event
        // Unfortunately the exact size of PcrEventInputs header is not exposed,
        // so we allocate a bit more than needed.
        let mut event_buf = alloc::vec![0u8; 64 + event_data.len()];
        let event = efi_tcg_v2::PcrEventInputs::new_in_buffer(
            &mut event_buf,
            uefi::proto::tcg::PcrIndex(pcr_index),
            uefi::proto::tcg::EventType(event_type),
            event_data,
        )
        .map_err(|err| err.to_err_without_payload())?;

        proto.hash_log_extend_event(extend_flags_to_efi(flags), data_to_hash, event)
    } else {
        debugln!("[efi] TCG2 protocol not available, skipping measurement");
        Ok(())
    }
}

/// Converts our ExtendFlags to the UEFI TCG2 HashLogExtendEvent flags.
fn extend_flags_to_efi(flags: ExtendFlags) -> efi_tcg_v2::HashLogExtendEventFlags {
    let mut efi_flags = efi_tcg_v2::HashLogExtendEventFlags::empty();
    if flags.contains(ExtendFlags::EXTEND_ONLY) {
        efi_flags |= efi_tcg_v2::HashLogExtendEventFlags::EFI_TCG2_EXTEND_ONLY;
    }
    if flags.contains(ExtendFlags::PE_COFF_IMAGE) {
        efi_flags |= efi_tcg_v2::HashLogExtendEventFlags::PE_COFF_IMAGE;
    }
    efi_flags
}

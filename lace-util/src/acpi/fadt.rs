// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! ACPI FADT (Fixed ACPI Description Table) and FACS parsing.

use zerocopy::{
    FromBytes, Immutable, KnownLayout,
    byteorder::{LE, U16, U32, U64},
};

/// ACPI Generic Address Structure (GAS).
#[repr(C, packed)]
#[derive(Clone, Copy, FromBytes, Immutable, KnownLayout)]
pub struct Gas {
    pub address_space_id: u8,
    pub register_bit_width: u8,
    pub register_bit_offset: u8,
    pub access_size: u8,
    pub address: U64<LE>,
}

/// ACPI FADT (Fixed ACPI Description Table), revision 3+ layout.
#[repr(C, packed)]
#[derive(FromBytes, Immutable, KnownLayout)]
pub struct Fadt {
    pub header: super::SdtHeader,
    pub firmware_ctrl: U32<LE>,
    pub dsdt: U32<LE>,
    pub reserved0: u8,
    pub preferred_pm_profile: u8,
    pub sci_int: U16<LE>,
    pub smi_cmd: U32<LE>,
    pub acpi_enable: u8,
    pub acpi_disable: u8,
    pub s4bios_req: u8,
    pub pstate_cnt: u8,
    pub pm1a_evt_blk: U32<LE>,
    pub pm1b_evt_blk: U32<LE>,
    pub pm1a_cnt_blk: U32<LE>,
    pub pm1b_cnt_blk: U32<LE>,
    pub pm2_cnt_blk: U32<LE>,
    pub pm_tmr_blk: U32<LE>,
    pub gpe0_blk: U32<LE>,
    pub gpe1_blk: U32<LE>,
    pub pm1_evt_len: u8,
    pub pm1_cnt_len: u8,
    pub pm2_cnt_len: u8,
    pub pm_tmr_len: u8,
    pub gpe0_blk_len: u8,
    pub gpe1_blk_len: u8,
    pub gpe1_base: u8,
    pub cst_cnt: u8,
    pub p_lvl2_lat: U16<LE>,
    pub p_lvl3_lat: U16<LE>,
    pub flush_size: U16<LE>,
    pub flush_stride: U16<LE>,
    pub duty_offset: u8,
    pub duty_width: u8,
    pub day_alrm: u8,
    pub mon_alrm: u8,
    pub century: u8,
    pub iapc_boot_arch: U16<LE>,
    pub reserved1: u8,
    pub flags: U32<LE>,
    pub reset_reg: Gas,
    pub reset_value: u8,
    pub reserved2: [u8; 3],
    pub x_firmware_ctrl: U64<LE>,
    pub x_dsdt: U64<LE>,
    pub x_pm1a_evt_blk: Gas,
    pub x_pm1b_evt_blk: Gas,
    pub x_pm1a_cnt_blk: Gas,
    pub x_pm1b_cnt_blk: Gas,
    pub x_pm2_cnt_blk: Gas,
    pub x_pm_tmr_blk: Gas,
    pub x_gpe0_blk: Gas,
    pub x_gpe1_blk: Gas,
}

/// ACPI FACS (Firmware ACPI Control Structure).
#[repr(C, packed)]
#[derive(FromBytes, Immutable, KnownLayout)]
pub struct Facs {
    pub signature: [u8; 4],
    pub length: U32<LE>,
    pub hardware_signature: U32<LE>,
    pub firmware_waking_vector: U32<LE>,
    pub global_lock: U32<LE>,
    pub flags: U32<LE>,
    pub x_firmware_waking_vector: U64<LE>,
    pub version: u8,
}

/// Extract the FACS physical address from a FADT.
pub fn parse_fadt_facs_addr(data: &[u8]) -> Option<u64> {
    let (fadt, _) = Fadt::ref_from_prefix(data).ok()?;
    if &fadt.header.signature != b"FACP" {
        return None;
    }

    // Prefer 64-bit X_FIRMWARE_CTRL if non-zero
    let addr = if fadt.x_firmware_ctrl != 0 {
        fadt.x_firmware_ctrl.get()
    } else {
        fadt.firmware_ctrl.get().into()
    };

    if addr == 0 { None } else { Some(addr) }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_fadt_facs_addr_32bit() {
        let mut data = [0u8; size_of::<Fadt>()];
        data[0..4].copy_from_slice(b"FACP");
        data[4..8].copy_from_slice(&(size_of::<Fadt>() as u32).to_le_bytes());
        // firmware_ctrl at offset 36
        let facs_addr: u32 = 0xDEAD_0000;
        data[36..40].copy_from_slice(&facs_addr.to_le_bytes());

        assert_eq!(parse_fadt_facs_addr(&data), Some(0xDEAD_0000));
    }

    #[test]
    fn test_parse_fadt_facs_addr_64bit() {
        let mut data = [0u8; size_of::<Fadt>()];
        data[0..4].copy_from_slice(b"FACP");
        data[4..8].copy_from_slice(&(size_of::<Fadt>() as u32).to_le_bytes());
        // firmware_ctrl (32-bit) at offset 36
        data[36..40].copy_from_slice(&0x1234_0000u32.to_le_bytes());
        // x_firmware_ctrl (64-bit)
        let x_offset = core::mem::offset_of!(Fadt, x_firmware_ctrl);
        data[x_offset..x_offset + 8].copy_from_slice(&0xCAFE_BABE_0000u64.to_le_bytes());

        // Should prefer x_firmware_ctrl
        assert_eq!(parse_fadt_facs_addr(&data), Some(0xCAFE_BABE_0000));
    }

    #[test]
    fn test_parse_fadt_bad_signature() {
        let mut data = [0u8; size_of::<Fadt>()];
        data[0..4].copy_from_slice(b"XXXX");
        assert_eq!(parse_fadt_facs_addr(&data), None);
    }

    #[test]
    fn test_parse_fadt_zero_facs() {
        let mut data = [0u8; size_of::<Fadt>()];
        data[0..4].copy_from_slice(b"FACP");
        data[4..8].copy_from_slice(&(size_of::<Fadt>() as u32).to_le_bytes());
        assert_eq!(parse_fadt_facs_addr(&data), None);
    }
}

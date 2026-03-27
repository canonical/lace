// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! BIOS interrupt call functionality.
//!
//! Provides a way to call BIOS interrupts from long mode.
//! Uses a thunk in real mode to perform the calls.
//! Requires buffers to be in low memory (<1MB).

use core::arch::global_asm;

#[repr(C, packed)]
#[derive(Debug, Default, Clone, Copy)]
pub struct BiosRegisters {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
    pub esi: u32,
    pub edi: u32,
    pub ebp: u32,
    pub ds: u16,
    pub es: u16,
    pub fs: u16,
    pub gs: u16,
    pub flags: u32,
}

impl BiosRegisters {
    pub fn new() -> Self {
        Self::default()
    }
}

unsafe extern "C" {
    fn bios_call_asm(int_num: u8);
    static mut bios_bounce_buffer: BiosRegisters;
}

/// Calls a BIOS interrupt with the given register state.
/// # Safety
/// This is unsafe in so many ways it is almost impossible to enumerate.
/// The caller must ensure to call a valid BIOS interrupt number with valid
/// register state for that specific interrupt, otherwise almost any arbitrary
/// undefined behaviour up-to and including a system crash can occur.
pub unsafe fn bios_call(int_num: u8, regs: &mut BiosRegisters) {
    unsafe {
        // Copy registers to bounce buffer
        bios_bounce_buffer = *regs;

        bios_call_asm(int_num);

        // Copy registers back from bounce buffer
        *regs = bios_bounce_buffer;
    }
}

global_asm!(include_str!("int_trampoline.s"), options(att_syntax));

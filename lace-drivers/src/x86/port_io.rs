// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only

//! AMD64 port based I/O wrapper routines

use core::arch::asm;

/// Read a byte from the given port.
///
/// # Safety
/// The caller must ensure the port corresponds to a valid device.
pub unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!("in al, dx", in("dx") port, out("al") value);
    }
    value
}

/// Write a byte to the given port.
///
/// # Safety
/// The caller must ensure the port corresponds to a valid device.
pub unsafe fn outb(port: u16, value: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value);
    }
}

/// Read a word from the given port.
///
/// # Safety
/// The caller must ensure the port corresponds to a valid device.
pub unsafe fn inw(port: u16) -> u16 {
    let value: u16;
    unsafe {
        asm!("in ax, dx", in("dx") port, out("ax") value);
    }
    value
}

/// Write a word to the given port.
///
/// # Safety
/// The caller must ensure the port corresponds to a valid device.
pub unsafe fn outw(port: u16, value: u16) {
    unsafe {
        asm!("out dx, ax", in("dx") port, in("ax") value);
    }
}

/// Read a double word from the given port.
///
/// # Safety
/// The caller must ensure the port corresponds to a valid device.
pub unsafe fn inl(port: u16) -> u32 {
    let value: u32;
    unsafe {
        asm!("in eax, dx", in("dx") port, out("eax") value);
    }
    value
}

/// Write a double word to the given port.
///
/// # Safety
/// The caller must ensure the port corresponds to a valid device.
pub unsafe fn outl(port: u16, value: u32) {
    unsafe {
        asm!("out dx, eax", in("dx") port, in("eax") value);
    }
}

/// I/O wait by writing to port 0x80.
///
/// # Safety
/// This writes to the debug port which is safe on standard x86 platforms.
pub unsafe fn io_wait() {
    unsafe {
        asm!("out 0x80, al", in("al") 0u8);
    }
}

// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! 8250/16550 UART driver using x86 port I/O

use super::port_io;
use core::fmt;

// Register offsets
const DATA: u16 = 0; // Data register
const IER: u16 = 1; // Interrupt enable register
const FCR: u16 = 2; // FIFO control register
const LCR: u16 = 3; // Line control register
const MCR: u16 = 4; // Modem control register
const LSR: u16 = 5; // Line status register

// Register offsets when DLAB (Divisor Latch Access Bit) is set
const DLL: u16 = 0; // Divisor latch low byte
const DLH: u16 = 1; // Divisor latch high byte

// Line status register bits
const LSR_DATA_READY: u8 = 0x01;
const LSR_TX_EMPTY: u8 = 0x20;

// Line control register bits
const LCR_8N1: u8 = 0x03; // 8 data bits, no parity, 1 stop bit
const LCR_DLAB: u8 = 0x80; // Divisor latch access bit

// UART clock and baud rate
const UART_CLOCK_HZ: u32 = 1_843_200;
const BAUD_RATE: u32 = 115_200;
const DIVISOR: u16 = (UART_CLOCK_HZ / (BAUD_RATE * 16)) as u16;

pub struct Uart8250 {
    base_port: u16,
}

impl Uart8250 {
    /// Create a new Uart8250 instance at the given base port.
    ///
    /// # Safety
    /// The caller must ensure that the given port actually corresponds to a UART8250 device.
    pub const unsafe fn new(base_port: u16) -> Self {
        Self { base_port }
    }

    /// Initialize the UART: disable interrupts, set 115200 baud 8N1, enable FIFO.
    pub fn init(&mut self) {
        unsafe {
            // Disable all interrupts
            port_io::outb(self.base_port + IER, 0x00);

            // Enable DLAB to set baud rate divisor
            port_io::outb(self.base_port + LCR, LCR_DLAB);

            // Set divisor for 115200 baud
            port_io::outb(self.base_port + DLL, DIVISOR as u8);
            port_io::outb(self.base_port + DLH, (DIVISOR >> 8) as u8);

            // 8 data bits, no parity, 1 stop bit (clears DLAB)
            port_io::outb(self.base_port + LCR, LCR_8N1);

            // Enable FIFO, clear TX/RX, 14-byte threshold
            port_io::outb(self.base_port + FCR, 0xC7);

            // Set RTS/DTR
            port_io::outb(self.base_port + MCR, 0x03);
        }
    }

    /// Read a byte from the UART (blocking).
    pub fn read_byte(&mut self) -> u8 {
        while (unsafe { port_io::inb(self.base_port + LSR) } & LSR_DATA_READY) == 0 {
            core::hint::spin_loop();
        }
        unsafe { port_io::inb(self.base_port + DATA) }
    }

    /// Write a byte to the UART (blocking).
    pub fn write_byte(&mut self, byte: u8) {
        while (unsafe { port_io::inb(self.base_port + LSR) } & LSR_TX_EMPTY) == 0 {
            core::hint::spin_loop();
        }
        unsafe { port_io::outb(self.base_port + DATA, byte) };
    }
}

impl fmt::Write for Uart8250 {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(byte);
        }
        Ok(())
    }
}

// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! BIOS Services for Printing

use super::int::{BiosRegisters, bios_call};
use core::fmt::Write;
use spin::mutex::Mutex;

/// Platform specific text output
pub fn print_impl(args: core::fmt::Arguments<'_>) {
    let mut output = OUTPUT.lock();
    write!(output, "{}", args).unwrap();
}

/// Platform specific text output with newline
pub fn println_impl(args: core::fmt::Arguments<'_>) {
    let mut output = OUTPUT.lock();
    writeln!(output, "{}", args).unwrap();
}

/// Global Output instance for BIOS text output
pub static OUTPUT: Mutex<Output> = Mutex::new(Output { _private: () });

/// Output struct implementing core::fmt::Write for BIOS text output
pub struct Output {
    _private: (),
}

impl core::fmt::Write for Output {
    fn write_char(&mut self, c: char) -> core::fmt::Result {
        if c == '\n' {
            self.write_char('\r')?;
        }
        let mut regs = BiosRegisters::new();
        regs.eax = 0x0E00 | (c as u32);
        regs.ebx = 0x0007; // Page 0, Color 7 (Light Grey)
        unsafe {
            bios_call(0x10, &mut regs);
        }
        Ok(())
    }

    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for c in s.chars() {
            self.write_char(c)?;
        }
        Ok(())
    }
}

/// Keyboard event structure containing character and scancode
#[derive(Debug, Clone, Copy)]
pub struct KeyEvent {
    pub char: char,
    pub scancode: u8,
}

/// Global Input instance for BIOS keyboard input
pub static INPUT: Mutex<Input> = Mutex::new(Input { _private: () });

/// Input struct for BIOS keyboard input
pub struct Input {
    _private: (),
}

impl Input {
    /// Read a single keystroke from the keyboard (blocking)
    pub fn read_key(&mut self) -> KeyEvent {
        let mut regs = BiosRegisters::new();
        regs.eax = 0x0000; // AH = 0x00 (Read Keystroke)
        unsafe {
            bios_call(0x16, &mut regs);
        }
        KeyEvent {
            char: (regs.eax & 0xFF) as u8 as char,
            scancode: ((regs.eax >> 8) & 0xFF) as u8,
        }
    }
}

/// Read a key from the keyboard
pub fn read_key() -> Result<KeyEvent, super::Error> {
    Ok(INPUT.lock().read_key())
}

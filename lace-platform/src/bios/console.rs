// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! BIOS console

use super::int::{BiosRegisters, bios_call};
use crate::console::{Input, InputEvent, Output};

/// BIOS console output
pub struct OutputImpl {
    _private: (),
}

impl OutputImpl {
    #[allow(clippy::new_without_default)]
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl core::fmt::Write for OutputImpl {
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

impl Output for OutputImpl {
    fn get_position(&mut self) -> Result<(usize, usize), crate::Error> {
        Ok((0, 0))
    }

    fn set_position(&mut self, _x: usize, _y: usize) -> Result<(), crate::Error> {
        Ok(())
    }

    fn clear_screen(&mut self) -> Result<(), crate::Error> {
        let mut regs = BiosRegisters::new();
        // INT 10h AH=00h AL=03h: set video mode 80x25 text (clears the screen).
        regs.eax = 0x0003;
        unsafe {
            bios_call(0x10, &mut regs);
        }
        Ok(())
    }
}

/// BIOS console input
pub struct InputImpl {
    _private: (),
}

impl InputImpl {
    #[allow(clippy::new_without_default)]
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl Input for InputImpl {
    /// Read a single keystroke from the keyboard (blocking)
    fn wait_input(&mut self) -> Result<InputEvent, crate::Error> {
        let mut regs = BiosRegisters::new();
        regs.eax = 0x0000; // AH = 0x00 (Read Keystroke)
        unsafe {
            bios_call(0x16, &mut regs);
        }
        Ok(InputEvent {
            char: (regs.eax & 0xFF) as u8 as char,
            scancode: ((regs.eax >> 8) & 0xFF) as u8,
        })
    }
}

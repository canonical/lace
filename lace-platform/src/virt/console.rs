// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Virt platform console (UART 8250 at 0x3F8)

use crate::console::{Input, InputEvent, Output};
use core::fmt::Write;
use lace_drivers::x86::uart8250::Uart8250;

pub struct OutputImpl(Uart8250);

impl OutputImpl {
    #[allow(clippy::new_without_default)]
    pub const fn new() -> Self {
        Self(unsafe { Uart8250::new(0x3F8) })
    }
}

impl core::fmt::Write for OutputImpl {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.0.write_str(s)
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
        // ANSI: clear screen + move cursor to home.
        self.0
            .write_str("\x1b[2J\x1b[H")
            .map_err(|_| crate::Error::Other)
    }
}

pub struct InputImpl(Uart8250);

impl InputImpl {
    #[allow(clippy::new_without_default)]
    pub const fn new() -> Self {
        Self(unsafe { Uart8250::new(0x3F8) })
    }
}

impl Input for InputImpl {
    fn wait_input(&mut self) -> Result<InputEvent, crate::Error> {
        let byte = self.0.read_byte();
        Ok(InputEvent {
            char: byte as char,
            scancode: 0,
        })
    }
}

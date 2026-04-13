// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! EFI Services for Printing and Input

use crate::console::{Input, InputEvent, Output};
use uefi::proto::console::text::{Key, ScanCode};

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
    fn write_str(&mut self, s: &str) -> Result<(), core::fmt::Error> {
        uefi::system::with_stdout(|stdout| stdout.write_str(s)).map_err(|_| core::fmt::Error)
    }
}

impl Output for OutputImpl {
    fn get_position(&mut self) -> Result<(usize, usize), crate::Error> {
        Ok(uefi::system::with_stdout(|stdout| stdout.cursor_position()))
    }

    fn set_position(&mut self, x: usize, y: usize) -> Result<(), crate::Error> {
        uefi::system::with_stdout(|stdout| stdout.set_cursor_position(x, y))?;
        Ok(())
    }

    fn clear_screen(&mut self) -> Result<(), crate::Error> {
        uefi::system::with_stdout(|stdout| stdout.clear())?;
        Ok(())
    }
}

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
    /// Wait for a single input event
    fn wait_input(&mut self) -> Result<InputEvent, crate::Error> {
        // NOTE: if this is NULL the firmware is sufficiently broken that we should just panic
        let wait_for_key_event =
            uefi::system::with_stdin(|stdin| stdin.wait_for_key_event()).unwrap();
        // Wait for input to be available
        uefi::boot::wait_for_event(&mut [wait_for_key_event])
            .map_err(|e| e.to_err_without_payload())?;
        // Read the key
        let key_opt = uefi::system::with_stdin(|stdin| stdin.read_key())?;
        match key_opt {
            Some(key) => {
                let (c, sc) = match key {
                    // The firmware did not provide scancodes for printable characters
                    Key::Printable(c) => (char::from(c), 0),
                    // Translate UEFI scancodes to Scancode set 1
                    // It is questionable if this is what we want to standardize on,
                    // maybe we should translate to Linux input layer codes instead?
                    Key::Special(ScanCode::NULL) => ('\0', 0),
                    Key::Special(ScanCode::UP) => ('\0', 0x48),
                    Key::Special(ScanCode::DOWN) => ('\0', 0x50),
                    Key::Special(ScanCode::RIGHT) => ('\0', 0x4D),
                    Key::Special(ScanCode::LEFT) => ('\0', 0x4B),
                    Key::Special(ScanCode::HOME) => ('\0', 0x47),
                    Key::Special(ScanCode::END) => ('\0', 0x4F),
                    Key::Special(ScanCode::INSERT) => ('\0', 0x52),
                    Key::Special(ScanCode::DELETE) => ('\x7F', 0x53), // DEL
                    Key::Special(ScanCode::PAGE_UP) => ('\0', 0x49),
                    Key::Special(ScanCode::PAGE_DOWN) => ('\0', 0x51),
                    Key::Special(ScanCode::FUNCTION_1) => ('\0', 0x3B),
                    Key::Special(ScanCode::FUNCTION_2) => ('\0', 0x3C),
                    Key::Special(ScanCode::FUNCTION_3) => ('\0', 0x3D),
                    Key::Special(ScanCode::FUNCTION_4) => ('\0', 0x3E),
                    Key::Special(ScanCode::FUNCTION_5) => ('\0', 0x3F),
                    Key::Special(ScanCode::FUNCTION_6) => ('\0', 0x40),
                    Key::Special(ScanCode::FUNCTION_7) => ('\0', 0x41),
                    Key::Special(ScanCode::FUNCTION_8) => ('\0', 0x42),
                    Key::Special(ScanCode::FUNCTION_9) => ('\0', 0x43),
                    Key::Special(ScanCode::FUNCTION_10) => ('\0', 0x44),
                    Key::Special(ScanCode::FUNCTION_11) => ('\0', 0x57),
                    Key::Special(ScanCode::FUNCTION_12) => ('\0', 0x58),
                    Key::Special(ScanCode::ESCAPE) => ('\0', 0x01),
                    // Pass along the firmware scan-code (likely not useful)
                    Key::Special(sc) => ('\0', sc.0 as u8),
                };
                Ok(InputEvent {
                    char: c,
                    scancode: sc,
                })
            }
            None => unreachable!(), // should not happen after wait_for_key_event
        }
    }
}

// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Mock console abstraction.

use crate::console::{Input, InputEvent, Output};

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
        std::io::Write::write_all(&mut std::io::stdout(), s.as_bytes())
            .map_err(|_| core::fmt::Error)?;
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
    fn wait_input(&mut self) -> Result<InputEvent, crate::Error> {
        Err(crate::Error)
    }
}

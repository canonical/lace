// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Mock console abstraction.

extern crate std;

use crate::Error;
use core::fmt;

pub fn print_impl(args: fmt::Arguments<'_>) {
    std::print!("{}", args);
}

pub fn println_impl(args: fmt::Arguments<'_>) {
    std::println!("{}", args);
}

#[derive(Debug, Clone, Copy)]
pub struct KeyEvent {
    pub char: char,
    pub scancode: u8,
}

pub fn read_key() -> Result<KeyEvent, Error> {
    todo!()
}

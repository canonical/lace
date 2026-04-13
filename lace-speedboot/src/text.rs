// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Julian Andres Klode <julian.klode@canonical.com>
//! Text-based user interface for menu display and input.

use alloc::boxed::Box;
use alloc::string::String;
use lace_platform::{print, println};

use crate::SpeedbootError;
use crate::bootflows::BootConfiguration;

/// Display the boot menu to the user.
pub fn display_menu(entries: &[Box<dyn BootConfiguration>]) {
    println!("Boot Menu:");
    println!("-----------");

    for (idx, entry) in entries.iter().enumerate() {
        println!("  [{}] {}", idx, entry.title());
    }

    println!("");
}

/// Get user selection from the menu.
pub fn get_user_selection(max: usize) -> Result<usize, SpeedbootError> {
    print!("Select boot entry (0-{}): ", max - 1);

    let mut selection_str = String::new();

    // Read input character by character
    loop {
        let input_event = lace_platform::console::Input::wait_input(
            &mut lace_platform::console::stdin(),
        )
        .map_err(|_| SpeedbootError::InvalidSelection)?;
        let c = input_event.char;

        if c == '\r' || c == '\n' {
            println!();
            break;
        } else if c == '\x08' {
            // Backspace
            if !selection_str.is_empty() {
                selection_str.pop();
                print!("\x08 \x08");
            }
        } else if c.is_ascii_digit() {
            selection_str.push(c);
            print!("{}", c);
        }
    }

    let selection: usize = selection_str
        .parse()
        .map_err(|_| SpeedbootError::InvalidSelection)?;

    if selection >= max {
        return Err(SpeedbootError::InvalidSelection);
    }

    Ok(selection)
}

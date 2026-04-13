// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! lace-platform console interfaces.
//!
//! Each platform defines `OutputImpl` / `InputImpl` concrete types that
//! forward to the underlying firmware console. They are held in static
//! mutexes at this level, so no allocation or explicit init step is
//! needed and the console is always available.
//!
//! Callers acquire the console via [`stdout`] / [`stdin`], which
//! return stateless handles implementing the [`Output`] / [`Input`]
//! traits. Each trait method takes the underlying mutex, performs one
//! driver call, and releases it. Holding the lock only across a single
//! driver call means arbitrary user code run between calls (e.g.
//! `Display` impls invoked by `writeln!`) never executes with the lock
//! held, so a panic from user code cannot leave the console mutex in
//! a locked state.

use crate::p::console::{InputImpl, OutputImpl};
use log::{LevelFilter, Log, Metadata, Record};
use spin::Mutex;

static STDOUT: Mutex<OutputImpl> = Mutex::new(OutputImpl::new());
static STDIN: Mutex<InputImpl> = Mutex::new(InputImpl::new());

/// Initialize the console subsystem.
pub fn init() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(if cfg!(debug_assertions) {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    });
}

/// `log::Log` implementation that writes formatted log records to stdout.
struct ConsoleLogger;

static LOGGER: ConsoleLogger = ConsoleLogger;

impl Log for ConsoleLogger {
    fn enabled(&self, _: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        use core::fmt::Write as _;
        let _ = writeln!(
            stdout(),
            "[{} {}] {}",
            record.level(),
            record.target(),
            record.args()
        );
    }

    fn flush(&self) {}
}

/// Handle to the platform stdout.
///
/// Direct access to stdout is intended for interactive UI (menu
/// rendering, cursor positioning, ...). General program output should
/// go through the `log` crate, which the console logger routes back
/// through here.
pub fn stdout() -> LockedConsole {
    LockedConsole
}

/// Handle to the platform stdin.
pub fn stdin() -> LockedInput {
    LockedInput
}

/// Stateless handle implementing [`Output`] by locking the console
/// mutex per call.
pub struct LockedConsole;

impl core::fmt::Write for LockedConsole {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        STDOUT.lock().write_str(s)
    }
}

impl Output for LockedConsole {
    fn get_position(&mut self) -> Result<(usize, usize), crate::Error> {
        STDOUT.lock().get_position()
    }

    fn set_position(&mut self, x: usize, y: usize) -> Result<(), crate::Error> {
        STDOUT.lock().set_position(x, y)
    }

    fn clear_screen(&mut self) -> Result<(), crate::Error> {
        STDOUT.lock().clear_screen()
    }
}

/// Stateless handle implementing [`Input`] by locking the console
/// mutex per call.
pub struct LockedInput;

impl Input for LockedInput {
    fn wait_input(&mut self) -> Result<InputEvent, crate::Error> {
        STDIN.lock().wait_input()
    }
}

/// Render the panic banner and halt forever.
///
/// Intended for bare-metal platforms that have no stderr; hosted runs
/// simply do not call this and let the normal `std` panic machinery
/// unwind or abort.
pub fn panic(info: &dyn core::fmt::Display) -> ! {
    use core::fmt::Write as _;
    log::error!("{}", info);
    let mut stdout = stdout();
    let _ = stdout.clear_screen();
    let _ = writeln!(stdout, "{0:=<80}", "");
    let _ = writeln!(stdout, "{: ^80}", "PANIC");
    let _ = writeln!(stdout, "{0:=<80}", "");
    let _ = writeln!(stdout);
    let _ = writeln!(stdout, "{}", info);
    let _ = writeln!(stdout);
    let _ = writeln!(stdout, "{0:-<80}", "");
    let _ = writeln!(stdout, "System halted.");
    loop {
        core::hint::spin_loop();
    }
}

/// Output driver
pub trait Output: core::fmt::Write {
    fn get_position(&mut self) -> Result<(usize, usize), crate::Error>;

    fn set_position(&mut self, x: usize, y: usize) -> Result<(), crate::Error>;

    fn clear_screen(&mut self) -> Result<(), crate::Error>;
}

/// Input driver
pub trait Input {
    /// Wait until an input event
    fn wait_input(&mut self) -> Result<InputEvent, crate::Error>;
}

/// Input event
pub struct InputEvent {
    pub char: char,
    pub scancode: u8,
}

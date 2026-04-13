// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! BIOS platform abstractions.

use crate::println;
use core::arch::global_asm;
use lace_util::Display;

pub mod console;
pub mod e820;
pub mod fs;
pub mod hwid;
pub mod int;
pub mod linux;
pub mod mem;
pub mod tpm2;

pub use console::{print_impl, println_impl};

#[derive(Debug, Display)]
pub enum Error {
    #[display("Disk error: {:?}")]
    Disk(fs::DiskError),
    #[display("Other error")]
    Other,
}

impl From<fs::DiskError> for Error {
    fn from(e: fs::DiskError) -> Self {
        Error::Disk(e)
    }
}

#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo) -> ! {
    // Force unlock console in case we panic while locked.
    unsafe { console::OUTPUT.force_unlock() };
    println!("PANIC: {}", info);
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn lace_platform_bios_entry() -> ! {
    mem::init();

    unsafe extern "Rust" {
        fn lace_app_main() -> Result<(), Error>;
    }

    match unsafe { lace_app_main() } {
        Ok(_) => (),
        Err(e) => println!("Error: {}", e),
    }

    // Just hang if we fail to boot
    #[allow(clippy::empty_loop)]
    loop {}
}

global_asm!(include_str!("start.s"), options(att_syntax));

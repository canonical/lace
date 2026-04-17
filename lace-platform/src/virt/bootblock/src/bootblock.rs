// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! Virt platform bootblock — loads firmware ELF from CBFS and enters long mode

extern crate alloc;

use core::arch::global_asm;
use core::fmt::Write;
use lace_util::cbfs::Cbfs;
use lace_util::elf64::{Elf64, PT_LOAD};
use linked_list_allocator::LockedHeap;

static CONSOLE: spin::Mutex<lace_drivers::x86::uart8250::Uart8250> =
    spin::Mutex::new(unsafe { lace_drivers::x86::uart8250::Uart8250::new(0x3F8) });

macro_rules! println {
    ($($arg:tt)*) => {{
        let _ = writeln!(CONSOLE.lock(), $($arg)*);
    }};
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    unsafe {
        CONSOLE.force_unlock();
    }
    println!("[PANIC] {}", info);
    loop {}
}

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Page table structures (identity map first 4GB using 2MB pages).
#[repr(C, align(4096))]
struct PageTables {
    pml4: [u64; 512],
    pdpt: [u64; 512],
    pd: [[u64; 512]; 4],
}

static mut PAGE_TABLES: PageTables = PageTables {
    pml4: [0; 512],
    pdpt: [0; 512],
    pd: [[0; 512]; 4],
};

unsafe fn setup_page_tables() {
    let pt = &raw mut PAGE_TABLES;

    unsafe {
        (*pt).pml4[0] = (&raw const (*pt).pdpt as u64) | 0x3;

        for i in 0..4 {
            (*pt).pdpt[i] = (&raw const (*pt).pd[i] as u64) | 0x3;
        }

        for i in 0..4 {
            for j in 0..512 {
                let addr = ((i * 512 + j) as u64) << 21;
                (*pt).pd[i][j] = addr | 0x83;
            }
        }
    }
}

/// 64-bit trampoline: reload segment selectors then jump to entry in EDI.
#[rustfmt::skip]
const TRAMPOLINE_CODE: [u8; 16] = [
    0x66, 0xB8, 0x10, 0x00, // mov $0x10, %ax
    0x8E, 0xD8,             // mov %ax, %ds
    0x8E, 0xC0,             // mov %ax, %es
    0x8E, 0xE0,             // mov %ax, %fs
    0x8E, 0xE8,             // mov %ax, %gs
    0x8E, 0xD0,             // mov %ax, %ss
    0xFF, 0xE7,             // jmp *%rdi
];

const TRAMPOLINE_ADDR: u32 = 0x8000;

unsafe fn enter_long_mode(entry: u64) -> ! {
    let pml4_addr = (&raw const PAGE_TABLES) as u32;

    unsafe {
        let dst =
            core::slice::from_raw_parts_mut(TRAMPOLINE_ADDR as *mut u8, TRAMPOLINE_CODE.len());
        dst.copy_from_slice(&TRAMPOLINE_CODE);
    }

    unsafe {
        core::arch::asm!(
            "mov {entry}, %edi",
            "mov {trampoline}, %esi",

            "mov %cr4, %eax",
            "or $0x20, %eax",
            "mov %eax, %cr4",

            "mov {pml4}, %cr3",

            "mov $0xC0000080, %ecx",
            "rdmsr",
            "or $0x100, %eax",
            "wrmsr",

            "mov %cr0, %eax",
            "or $0x80000000, %eax",
            "mov %eax, %cr0",

            "pushl $0x08",
            "pushl %esi",
            "lretl",

            pml4 = in(reg) pml4_addr,
            entry = in(reg) entry as u32,
            trampoline = in(reg) TRAMPOLINE_ADDR,
            options(noreturn, att_syntax),
        );
    }
}

fn get_rom_slice() -> (&'static [u8], u32) {
    let header_ptr_addr = 0xFFFF_FFFCu32 as *const u32;
    let header_addr = unsafe { core::ptr::read_unaligned(header_ptr_addr) };
    let romsize_addr = (header_addr + 8) as *const [u8; 4];
    let romsize = u32::from_be_bytes(unsafe { core::ptr::read(romsize_addr) });
    let rom_base = 0u32.wrapping_sub(romsize);
    let rom = unsafe { core::slice::from_raw_parts(rom_base as *const u8, romsize as usize) };
    (rom, rom_base)
}

#[unsafe(export_name = "bootblock_main")]
fn main() {
    unsafe extern "C" {
        static mut _heap_start: u8;
    }
    unsafe {
        ALLOCATOR.lock().init(&raw mut _heap_start, 1024 * 1024);
    }

    println!("Bootblock started!");

    let (rom, rom_base) = get_rom_slice();
    let cbfs = Cbfs::parse(rom, rom_base).expect("CBFS not found");

    let firmware = cbfs
        .find_file("fallback/payload")
        .expect("firmware not found in CBFS");
    println!("Loading firmware ({} bytes)", firmware.data.len());

    let elf = Elf64::parse(firmware.data).expect("failed to parse firmware ELF");
    println!("ELF entry point: {:#x}", elf.entry());

    elf.for_each_phdr(|phdr| {
        if phdr.p_type == PT_LOAD {
            let paddr = phdr.p_paddr as usize;
            let filesz = phdr.p_filesz as usize;
            let memsz = phdr.p_memsz as usize;

            println!(
                "  LOAD paddr={:#x} filesz={:#x} memsz={:#x}",
                paddr, filesz, memsz
            );

            if filesz > 0 {
                let src = elf.segment_data(phdr).expect("segment data out of bounds");
                let dst = unsafe { core::slice::from_raw_parts_mut(paddr as *mut u8, filesz) };
                dst.copy_from_slice(src);
            }

            if memsz > filesz {
                let bss = unsafe {
                    core::slice::from_raw_parts_mut((paddr + filesz) as *mut u8, memsz - filesz)
                };
                bss.fill(0);
            }
        }
        true
    })
    .expect("failed to iterate program headers");

    println!("Entering long mode...");
    unsafe {
        setup_page_tables();
        enter_long_mode(elf.entry());
    }
}

global_asm!(include_str!("reset.s"), options(att_syntax));

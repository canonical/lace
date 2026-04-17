// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Virtual machine platform (no firmware)

use core::arch::global_asm;
use lace_util::Display;
use zerocopy::FromBytes;

use crate::mem::PageAllocationConstraint;
use crate::memmap::{MemoryMap, MemoryType, PAGE_SIZE};

pub mod console;
pub mod fs;
pub mod hwid;
pub mod linux;
pub mod mem;
pub mod tpm2;

/// ACPI RSDP physical address, set during platform init.
pub(crate) static mut RSDP_ADDR: u64 = 0;

/// Address where the wakeup trampoline is installed in low RAM.
/// This page is rewritten on every boot (cold or resume), so it does not
/// need to be preserved across the OS run.
const WAKEUP_BASE: usize = 0x1000;

/// Fixed physical address holding the saved FACS pointer for S3 resume.
///
/// The enclosing page is marked AcpiNvs in the e820 we hand to the OS, since
/// AcpiNvs is the only memory type guaranteed to survive S3 untouched.
const FACS_SAVE_ADDR: usize = 0x2000;

/// Physical range of bootblock RAM (data, BSS, stack before firmware
/// takes over). Must be preserved across S3.
const BOOTBLOCK_RAM_BASE: u64 = 0x0100_0000; // 16 MiB
const BOOTBLOCK_RAM_SIZE: u64 = 0x0010_0000; // 1 MiB

// Physical extent of the firmware ELF image. Defined by the linker
// script (`x86_64-virt.ld`), which sits it above the bootblock RAM
// so low memory stays free.
#[cfg(target_arch = "x86_64")]
unsafe extern "C" {
    static _firmware_start: u8;
    static _firmware_end: u8;
}

#[derive(Debug, Display)]
pub enum Error {
    #[display("Virtio error")]
    Virtio,
    #[display("Boot error")]
    Boot,
    #[display("Other error")]
    Other,
}

/// Allocator callback handed to `fw_cfg::load_acpi_tables`. Each ACPI
/// table comes straight from the page allocator as `AcpiNvs`, so the
/// OS sees it preserved across handoff without the firmware needing to
/// walk the tables afterwards to mark them.
///
/// Alignments above `PAGE_SIZE` are handled by the `PageAllocation`
/// over-allocation path; the caller's requested `size` is returned to
/// the driver as a slice, while the surrounding page slack is still
/// covered by the same allocation (and collapses to `AcpiNvs` on the
/// e820 export).
#[cfg(target_arch = "x86_64")]
fn alloc_acpi_table(size: usize, align: usize) -> &'static mut [u8] {
    use crate::mem::{PageAllocation, PageAllocationIface};
    let pages = size.div_ceil(PAGE_SIZE as usize);
    let alignment = if align > PAGE_SIZE as usize {
        Some(align)
    } else {
        None
    };
    let alloc = PageAllocation::new_zeroed(
        PageAllocationConstraint::AnyAddress,
        Some(MemoryType::AcpiNvs),
        pages,
        alignment,
    )
    .expect("ACPI table page allocation failed");
    let (ptr, _pages) = alloc.into_raw();
    // SAFETY: `ptr` owns `pages * PAGE_SIZE` bytes of AcpiNvs memory;
    // the driver writes exactly `size` bytes into it, and the surrounding
    // page slack stays within the same AcpiNvs reservation.
    unsafe { core::slice::from_raw_parts_mut(ptr.as_ptr(), size) }
}

/// Reserve a page-aligned byte range by re-typing it away from Usable.
/// Panics if the range is not currently Usable, since platform init has
/// no recovery path if the e820 does not contain the memory we need.
#[cfg(target_arch = "x86_64")]
fn reserve(map: &mut MemoryMap, base: u64, bytes: u64, type_: MemoryType) {
    let aligned_base = base & !(PAGE_SIZE - 1);
    let aligned_end = (base + bytes).next_multiple_of(PAGE_SIZE);
    let pages = ((aligned_end - aligned_base) / PAGE_SIZE) as usize;
    if let Err(e) = map.allocate(
        PageAllocationConstraint::FixedAddress(aligned_base),
        type_,
        pages,
        None,
    ) {
        panic!(
            "platform reservation [{:#x}, {:#x}) as {:?} failed: {}",
            aligned_base, aligned_end, type_, e
        );
    }
}

/// Firmware-internal physical reservations. These get re-typed away from
/// Usable before the Rust heap is carved, so the heap can't land on
/// them. The firmware ELF load region and bootblock RAM are where our
/// own code and data live; the wakeup trampoline page and FACS save
/// page are fixed low-memory slots used by S3.
#[cfg(target_arch = "x86_64")]
fn reserve_firmware_regions(map: &mut MemoryMap) {
    let fw_start = (&raw const _firmware_start) as u64;
    let fw_end = (&raw const _firmware_end) as u64;
    reserve(map, BOOTBLOCK_RAM_BASE, BOOTBLOCK_RAM_SIZE, MemoryType::Reserved);
    reserve(map, fw_start, fw_end - fw_start, MemoryType::Reserved);
    reserve(map, WAKEUP_BASE as u64, PAGE_SIZE, MemoryType::Reserved);
    reserve(map, FACS_SAVE_ADDR as u64, PAGE_SIZE, MemoryType::AcpiNvs);
}

#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo) -> ! {
    crate::console::panic(info)
}

#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
pub extern "C" fn lace_platform_virt_entry() -> ! {
    // Q35 chipset initialization via legacy PCI config space.
    // This must happen early, before S3 detection and ACPI table loading.
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        use lace_drivers::x86::pci_legacy::{read_u16, read_u32, write_u8, write_u32};

        // Detect Q35 MCH (BDF 0:0:0, device ID 0x29C0)
        let mch_dev_id = read_u16(0, 0, 0, 0x02);
        if mch_dev_id == 0x29C0 {
            // Enable PCIEXBAR (ECAM) - MCH register at offset 0x60 (64-bit)
            // Read the default base address programmed by QEMU and set the enable bit
            let pciexbar_lo = read_u32(0, 0, 0, 0x60);
            let pciexbar_hi = read_u32(0, 0, 0, 0x64);
            write_u32(0, 0, 0, 0x60, pciexbar_lo | 1);
            write_u32(0, 0, 0, 0x64, pciexbar_hi);

            // Program ICH9 LPC (BDF 0:1f:0) ACPI base address
            // PMBASE register at offset 0x40, ACPI control at offset 0x44
            write_u32(0, 0x1f, 0, 0x40, 0x600 | 1); // PMBASE = 0x600, bit 0 = I/O enable
            write_u8(0, 0x1f, 0, 0x44, 0x80); // ACPI_CNTL bit 7 = ACPI enable
        }
    }

    // Check for S3 resume before doing any further initialization.
    // QEMU signals S3 resume via CMOS register 0x0F == 0xFE.
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        let cmos_reason = unsafe {
            lace_drivers::x86::port_io::outb(0x70, 0x0F);
            lace_drivers::x86::port_io::inb(0x71)
        };

        if cmos_reason == 0xFE {
            // Clear the shutdown reason so a failed resume does not bootloop.
            unsafe {
                lace_drivers::x86::port_io::outb(0x70, 0x0F);
                lace_drivers::x86::port_io::outb(0x71, 0x00);
            }
            install_wakeup_trampoline();
            s3_resume();
        }
    }

    // Normal boot path:
    //   1. Probe fw_cfg (no alloc) and stream etc/e820 into the map.
    //   2. Reserve firmware-occupied memory so the heap can't land there.
    //   3. Carve the Rust heap from what's left, install the allocator.
    //   4. Use alloc-hungry drivers (ACPI table loader, PCI enumeration).
    crate::console::init();
    let fw_cfg = lace_drivers::fw_cfg::FwCfg::probe().expect("fw_cfg not found");
    mem::read_e820(&fw_cfg);
    crate::memmap::with_memory_map(reserve_firmware_regions);
    mem::init_heap();

    // Load ACPI tables from fw_cfg. Each table is allocated directly as
    // AcpiNvs from the page allocator, so they're preserved across OS
    // handoff without any post-hoc walk-and-reserve step.
    let rsdp_region = fw_cfg
        .load_acpi_tables(alloc_acpi_table)
        .expect("failed to load ACPI tables");
    unsafe { RSDP_ADDR = rsdp_region.as_ptr() as u64 };

    // Parse RSDP and find PCI ECAM base from MCFG
    let rsdp = lace_util::acpi::Rsdp::parse(rsdp_region);
    if let Some(ref rsdp) = rsdp {
        let acpi_deref =
            |addr: u64, len: usize| unsafe { core::slice::from_raw_parts(addr as *const u8, len) };

        // Resolve the FACS physical address from FADT and stash it in the
        // AcpiNvs save page so the S3 resume path can find it without
        // re-walking heap-allocated tables.
        if let Some(fadt_addr) = rsdp.find_table(b"FACP", acpi_deref) {
            let hdr_bytes = acpi_deref(
                fadt_addr,
                core::mem::size_of::<lace_util::acpi::SdtHeader>(),
            );
            if let Ok((hdr, _)) = lace_util::acpi::SdtHeader::ref_from_prefix(hdr_bytes)
                && let Some(facs_addr) = lace_util::acpi::fadt::parse_fadt_facs_addr(acpi_deref(
                    fadt_addr,
                    hdr.length as usize,
                ))
            {
                unsafe {
                    core::ptr::write_volatile(FACS_SAVE_ADDR as *mut u64, facs_addr);
                }
            }
        }

        log::debug!(
            "ACPI: RSDP revision {}, RSDT {:#x}, XSDT {:#x}",
            rsdp.revision,
            rsdp.rsdt_address,
            rsdp.xsdt_address
        );

        if let Some(mcfg_addr) = rsdp.find_table(b"MCFG", acpi_deref)
            && let Some(mcfg_entry) = unsafe { lace_util::acpi::mcfg::parse_mcfg(mcfg_addr) }
        {
            let ecam_base = mcfg_entry.base_address;
            log::debug!("ACPI: PCI ECAM at {:#x}", ecam_base);
            fs::set_ecam_base(ecam_base);

            // Enumerate PCI bus and assign BAR addresses
            let ecam = lace_drivers::pci::Ecam::new(ecam_base);
            let devices = lace_drivers::pci::enumerate_bus(&ecam, 0);

            // PCI MMIO window: from the top of RAM (including any ACPI
            // NVS / reclaim carve-outs) up to the ECAM base.
            let mmio_start = crate::memmap::with_memory_map(|m| m.ram_end_below(ecam_base));
            let mut bar_alloc = lace_drivers::pci::BarAllocator::new(mmio_start, ecam_base);
            bar_alloc.assign_bars(&ecam, &devices);

            for dev in &devices {
                log::debug!(
                    "PCI {:02x}:{:02x}.{} [{:#06x}:{:#06x}]",
                    dev.bus,
                    dev.dev,
                    dev.func,
                    dev.vendor_id,
                    dev.device_id
                );
            }
        }
    }

    unsafe extern "Rust" {
        fn lace_app_main() -> Result<(), Error>;
    }

    if let Err(e) = unsafe { lace_app_main() } {
        log::error!("{}", e);
    }

    #[allow(clippy::empty_loop)]
    loop {}
}

/// Copy the wakeup trampoline to low RAM so it is available for S3 resume.
#[cfg(target_arch = "x86_64")]
fn install_wakeup_trampoline() {
    unsafe extern "C" {
        static wakeup_trampoline: u8;
        static wakeup_trampoline_end: u8;
    }
    unsafe {
        let start = &raw const wakeup_trampoline;
        let end = &raw const wakeup_trampoline_end;
        let size = end.offset_from(start) as usize;
        let src = core::slice::from_raw_parts(start, size);
        let dst = core::slice::from_raw_parts_mut(WAKEUP_BASE as *mut u8, size);
        dst.copy_from_slice(src);
    }
}

/// Perform S3 resume: find the FACS waking vector and jump to it via the
/// wakeup trampoline.
///
/// We do not initialize the heap, read fw_cfg, or rebuild the e820 here.
/// The only state we rely on is the FACS pointer that the previous cold
/// boot saved into the AcpiNvs page at FACS_SAVE_ADDR, plus whatever the
/// OS preserved in its own AcpiNvs allocations.
#[cfg(target_arch = "x86_64")]
fn s3_resume() -> ! {
    // Read the FACS physical address from the AcpiNvs save slot.
    let facs_addr = unsafe { core::ptr::read_volatile(FACS_SAVE_ADDR as *const u64) };
    if facs_addr == 0 {
        panic!("S3 resume: FACS address not saved");
    }

    // Read the FACS (still in OS-preserved memory from before sleep) and
    // pick the appropriate waking vector.
    let facs_data = unsafe {
        core::slice::from_raw_parts(
            facs_addr as *const u8,
            core::mem::size_of::<lace_util::acpi::fadt::Facs>(),
        )
    };
    let (facs, _) =
        lace_util::acpi::fadt::Facs::ref_from_prefix(facs_data).expect("S3 resume: invalid FACS");

    if facs.x_firmware_waking_vector != 0 {
        // 64-bit waking vector: jump directly in long mode.
        let wake_vec = facs.x_firmware_waking_vector;
        unsafe {
            core::arch::asm!(
                "jmp {vec}",
                vec = in(reg) wake_vec,
                options(noreturn),
            );
        }
    } else if facs.firmware_waking_vector != 0 {
        // 32-bit waking vector: use the real-mode trampoline. Patch the
        // far pointer with the waking vector as segment:offset from long
        // mode, where memory access is straightforward.
        let wake_vec = facs.firmware_waking_vector;
        let segment = (wake_vec >> 4) as u16;
        let offset = (wake_vec & 0xF) as u16;
        unsafe extern "C" {
            static wakeup_trampoline: u8;
            static wakeup_far_ptr: u8;
        }
        unsafe {
            let tramp_start = &raw const wakeup_trampoline as usize;
            let far_ptr_off = &raw const wakeup_far_ptr as usize - tramp_start;
            let far_ptr = (WAKEUP_BASE + far_ptr_off) as *mut u16;
            core::ptr::write_unaligned(far_ptr, offset);
            core::ptr::write_unaligned(far_ptr.add(1), segment);

            core::arch::asm!(
                "jmp {base}",
                base = in(reg) WAKEUP_BASE,
                options(noreturn),
            );
        }
    } else {
        panic!("S3 resume: waking vector is zero");
    }
}

#[cfg(target_arch = "x86_64")]
global_asm!(include_str!("start.s"), options(att_syntax));
#[cfg(target_arch = "x86_64")]
global_asm!(include_str!("wakeup.s"), options(att_syntax));

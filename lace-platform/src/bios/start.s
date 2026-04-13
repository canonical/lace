# SPDX-License-Identifier: GPL-2.0-only

# Long mode startup code
# This code is entered after switching to long mode, but everything else
# (GDT, IDT, page tables, stack) is still the temporary ones set up in the
# 16-bit bootstrap.
# This code sets up the permanent GDT, IDT, page tables, and stack
# for the core long mode environment.

.globl  _start, gdt, gdtr, idtr, stack, stack_top, pml4, pdp, pd

.include "lace-platform/src/bios/defs.s"

.text
.code64

# Entry point
_start:

    # Load the permanent GDT
    lgdt gdtr

    # Load the empty IDT
    lidt idtr

    # Setup permanent page tables
    # Map first PML4 entry to PDP table
    lea pdp, %rdi
    or $0x3, %rdi          # Present + Writable
    lea pml4, %rax
    mov %rdi, (%rax)

    # Map first PDP entry to PD table
    lea pd, %rdi
    or $0x3, %rdi          # Present + Writable
    lea pdp, %rax
    mov %rdi, (%rax)

    # Map the 512 entries of the PD table to physical addresses 0..1GiB
    # using 2MiB huge pages
    lea pd, %rdi     # Destination address (PD table)
    mov $0x83, %rax        # Start physical address 0 | Present | Writable | Huge Page
    mov $512, %rcx         # Number of entries

1:
    mov %rax, (%rdi)       # Write entry
    add $0x200000, %rax    # Advance physical address by 2MiB
    add $8, %rdi           # Advance table pointer
    loop 1b                # Loop 512 times

    # Load CR3 with the address of PML4
    lea pml4, %rax
    mov %rax, %cr3

    # Load stack pointer
    lea stack_top, %rsp

    # Reload 64-bit code segment selector
    pushq $SEG_CODE64
    lea 1f, %rax
    pushq %rax
    lretq
1:
    # Reload 64-bit data segment selector
    mov $SEG_DATA64, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss

    # Call the BIOS entry point in Rust
    call lace_platform_bios_entry

    # Hang here if main returns
hang:
    hlt
    jmp hang

.data

# Global Descriptor Table
.align 16
gdt:
    .quad 0
    .quad 0x00209A0000000000 # 0x08: 64-bit Code
    .quad 0x0000920000000000 # 0x10: 64-bit Data
    .quad 0x00CF9A000000FFFF # 0x18: 32-bit Code
    .quad 0x00CF92000000FFFF # 0x20: 32-bit Data
    .quad 0x00009A010000FFFF # 0x28: 16-bit Code (Base 0x10000)
    .quad 0x000092010000FFFF # 0x30: 16-bit Data (Base 0x10000)

# Global Descriptor Table Register
gdtr:
    .word . - gdt - 1
    .quad gdt

# Interrupt Descriptor Table Register (empty)
idtr:
    .word 0
    .quad 0

.bss

# Stack
.align 16
stack:
    .space 8192 # 8 KB stack
stack_top:

# Page tables
.align 4096
pml4:
    .space 4096

.align 4096
pdp:
    .space 4096

.align 4096
pd:
    .space 4096

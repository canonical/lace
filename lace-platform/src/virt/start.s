# SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only

# Virt platform entry point
#
# Entered from the bootblock in 64-bit long mode with:
# - Bootblock's identity-mapped page tables (will be replaced)
# - Bootblock's GDT (will be replaced)
# - Interrupts disabled
# - No stack

.globl _start, gdt, gdtr

.text
.code64

_start:
    # Load our own GDT
    lgdt gdtr(%rip)

    # Reload code segment
    pushq $0x08
    lea 1f(%rip), %rax
    pushq %rax
    lretq
1:
    # Reload data segments
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss

    # Set up our own page tables (identity map first 4GB using 2MB pages)

    # PML4[0] -> PDPT
    lea pdpt(%rip), %rdi
    or $0x3, %rdi           # Present + Writable
    lea pml4(%rip), %rax
    mov %rdi, (%rax)

    # PDPT[0..3] -> PD[0..3]
    lea pd(%rip), %rdi
    lea pdpt(%rip), %rax
    mov $4, %rcx
2:
    mov %rdi, %rdx
    or $0x3, %rdx
    mov %rdx, (%rax)
    add $0x1000, %rdi       # Next PD table
    add $8, %rax            # Next PDPT entry
    loop 2b

    # Fill PD entries: 4 * 512 = 2048 entries, each mapping 2MB
    lea pd(%rip), %rdi
    mov $0x83, %rax         # Present + Writable + Page Size (2MB)
    mov $2048, %rcx
3:
    mov %rax, (%rdi)
    add $0x200000, %rax     # Next 2MB physical address
    add $8, %rdi            # Next PD entry
    loop 3b

    # Load CR3 with our PML4
    lea pml4(%rip), %rax
    mov %rax, %cr3

    # Set up stack
    lea stack_top(%rip), %rsp

    # Call the virt platform entry point
    call lace_platform_virt_entry

    # Should not return, but hang if it does
    cli
1:
    hlt
    jmp 1b

.section .rodata

# Global Descriptor Table
.align 16
gdt:
    .quad 0                    # 0x00: Null
    .quad 0x00209A0000000000   # 0x08: 64-bit Code
    .quad 0x0000920000000000   # 0x10: 64-bit Data

# Global Descriptor Table Register
gdtr:
    .word . - gdt - 1
    .quad gdt

.bss

# Stack
.align 16
stack:
    .space 65536 # 64 KB
stack_top:

# Page tables (identity map first 4GB)
.align 4096
pml4:
    .space 4096

.align 4096
pdpt:
    .space 4096

# 4 page directories (one per GB)
.align 4096
pd:
    .space 4096 * 4

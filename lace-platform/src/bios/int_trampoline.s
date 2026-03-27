# SPDX-License-Identifier: GPL-2.0-only

# BIOS Interrupt Trampoline Assembly
# This code switches the CPU from Long Mode to Real Mode,
# performs a BIOS interrupt call, and then switches back to Long Mode.

.global bios_call_asm

.include "lace-platform/src/bios/defs.s"

.section .text16, "awx"

# ------------------------------------------------------------------------------
# bios_call_asm(int_num: u8)
# Real Mode registers are passed via `bios_bounce_buffer` in low-memory.
# ------------------------------------------------------------------------------
bios_call_asm:
    push %rbx
    push %rbp
    push %r12
    push %r13
    push %r14
    push %r15

    # Save RSP
    mov %rsp, save_rsp


    # Patch INT instruction with interrupt number from RDI (DIL)
    movb %dil, int_instruction + 1

    # Jump to 32-bit Code Segment
    pushq $SEG_CODE32
    leaq .mode32, %rax
    pushq %rax
    lretq

.code32
.mode32:
    # Load 32-bit Data Segments
    mov $SEG_DATA32, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    mov %ax, %fs
    mov %ax, %gs

    # Disable paging in CR0
    mov %cr0, %eax
    and $0x7FFFFFFF, %eax
    mov %eax, %cr0

    # Disable Long Mode in EFER
    mov $0xC0000080, %ecx
    rdmsr
    and $0xFFFFFEFF, %eax
    wrmsr

    # Jump to 16-bit Code Segment (Target Offset = .mode16 - 0x10000)
    ljmpw $SEG_CODE16, $.mode16 - 0x10000

.code16
.mode16:
    # Load 16-bit Data Segments
    mov $SEG_DATA16, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    mov %ax, %fs
    mov %ax, %gs

    # Disable Protected Mode in CR0
    mov %cr0, %eax
    and $0xFFFFFFFE, %eax
    mov %eax, %cr0

    # Far jump to reload CS to Real Mode segment 0x1000
    # This ensures CS base is 0x10000 and CS selector is 0x1000
    ljmpw $0x1000, $.real_mode_entry - 0x10000

.real_mode_entry:
    # Setup Real Mode data segments
    mov %cs, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    mov %ax, %fs
    mov %ax, %gs

    # Setup Real Mode stack
    mov $rm_stack_top - 0x10000, %sp

    # Load Real Mode IDT (IVT at 0x0000)
    lidt rm_idtr - 0x10000

    # Load Registers
    mov $(bios_bounce_buffer - 0x10000), %bp # Lower 16 bits (relative to 0x10000)

    mov %ss:0(%bp), %eax
    mov %ss:4(%bp), %ebx
    mov %ss:8(%bp), %ecx
    mov %ss:12(%bp), %edx
    mov %ss:16(%bp), %esi
    mov %ss:20(%bp), %edi

    mov %ss:28(%bp), %ds
    mov %ss:30(%bp), %es
    mov %ss:32(%bp), %fs
    mov %ss:34(%bp), %gs

    pushl %ss:24(%bp)
    popl %ebp

    # Enable interrupts
    sti

    # Call BIOS Interrupt (with patched int number)
int_instruction:
    int $0x00

    # Disable interrupts
    cli

    # Save registers
    pushl %ebp

    mov $(bios_bounce_buffer - 0x10000), %bp # Lower 16 bits (relative to 0x10000)

    pushf
    pop %ss:36(%bp)

    mov %eax, %ss:0(%bp)
    mov %ebx, %ss:4(%bp)
    mov %ecx, %ss:8(%bp)
    mov %edx, %ss:12(%bp)
    mov %esi, %ss:16(%bp)
    mov %edi, %ss:20(%bp)

    mov %ds, %ss:28(%bp)
    mov %es, %ss:30(%bp)
    mov %fs, %ss:32(%bp)
    mov %gs, %ss:34(%bp)

    popl %eax
    mov %eax, %ss:24(%bp)

    # Enable Protected Mode in CR0
    mov %cr0, %eax
    or $1, %eax
    mov %eax, %cr0

    # Jump to 32-bit Code Segment
    ljmpl $SEG_CODE32, $.pmode32

.code32
.pmode32:

    # Reload 32-bit Data Segments
    mov $SEG_DATA32, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    mov %ax, %fs
    mov %ax, %gs

    # Enable Long Mode in EFER
    mov $0xC0000080, %ecx
    rdmsr
    or $0x100, %eax
    wrmsr

    # Enable paging in CR0
    mov %cr0, %eax
    or $0x80000000, %eax
    mov %eax, %cr0

    # Jump to 64-bit Code Segment
    ljmpl $SEG_CODE64, $.long_mode

.code64
.long_mode:

    # Reload 64-bit Data Segments
    mov $SEG_DATA64, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    mov %ax, %fs
    mov %ax, %gs

    # Restore IDTR
    lidt idtr

    # Restore RSP
    mov save_rsp, %rsp

    pop %r15
    pop %r14
    pop %r13
    pop %r12
    pop %rbp
    pop %rbx

    ret

// .section .data16

# Saved State Storage
.align 8
save_rsp:      .quad 0

.global bios_bounce_buffer
.align 16
bios_bounce_buffer:
    .space 64

# Real Mode IDTR (IVT at 0x0000)
.align 4
rm_idtr:
    .word 0x3FF
    .long 0

// .section .bss16

# Real mode stack
.align 16
rm_stack:
    .space 4096
rm_stack_top:


# S3 resume wakeup trampoline
#
# This code is copied to WAKEUP_BASE (0x1000) at boot time. On S3 resume,
# the firmware jumps here with the OS waking vector in %edi.
#
# It transitions from 64-bit long mode back to 16-bit real mode, then
# far-jumps to the OS waking vector.
#
# All internal addresses are computed relative to WAKEUP_BASE since
# the code runs from there, not from its link address.

.set WAKEUP_BASE, 0x1000
# Segment base for 16-bit segments (same as WAKEUP_BASE)
.set SEG16_BASE, 0x1000

.section .rodata

.global wakeup_trampoline, wakeup_trampoline_end, wakeup_far_ptr

wakeup_trampoline:
    .code64

    # Save waking vector
    mov %edi, %ebx

    # Load the GDT embedded in this trampoline
    lgdt (WAKEUP_BASE + wakeup_gdt_ptr - wakeup_trampoline)

    # Jump to 32-bit compatibility mode
    pushq $0x08
    pushq $(WAKEUP_BASE + .Lcompat - wakeup_trampoline)
    lretq

.Lcompat:
    .code32

    # Load 32-bit data segments
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    mov %ax, %fs
    mov %ax, %gs

    # Disable paging
    mov %cr0, %eax
    and $0x7FFFFFFF, %eax
    mov %eax, %cr0

    # Disable long mode in EFER
    mov $0xC0000080, %ecx
    rdmsr
    and $0xFFFFFEFF, %eax
    wrmsr

    # Clear CR3 and disable PAE in CR4
    xor %eax, %eax
    mov %eax, %cr3
    mov %cr4, %eax
    and $0xFFFFFFDF, %eax
    mov %eax, %cr4

    # Jump to 16-bit protected mode code segment (base = SEG16_BASE)
    ljmpw $0x18, $(.Lpm16 - wakeup_trampoline)

.Lpm16:
    .code16

    # Load 16-bit data segments (base = SEG16_BASE)
    mov $0x20, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    mov %ax, %fs
    mov %ax, %gs

    # Disable protected mode
    mov %cr0, %eax
    and $0xFFFFFFFE, %eax
    mov %eax, %cr0

    # Far jump to real mode with CS = SEG16_BASE >> 4
    ljmpw $(SEG16_BASE >> 4), $(.Lrm - wakeup_trampoline)

.Lrm:
    # Zero all segment registers for real mode
    xor %ax, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss

    # Load real mode IVT (DS=0 so use absolute address)
    lidt (WAKEUP_BASE + .Lrm_idtr - wakeup_trampoline)

    # Indirect far jump to OS waking vector via pointer in low memory
    ljmpw *(WAKEUP_BASE + wakeup_far_ptr - wakeup_trampoline)

wakeup_far_ptr:
    .word 0x0000    # offset
    .word 0x0000    # segment

.align 4
.Lrm_idtr:
    .word 0x3FF
    .long 0

    .align 16
wakeup_gdt:
    .quad 0                      # 0x00: null
    .quad 0x00CF9A000000FFFF     # 0x08: 32-bit code
    .quad 0x00CF92000000FFFF     # 0x10: 32-bit data
    # 16-bit code: base = SEG16_BASE, limit = 0xFFFF
    .word 0xFFFF                 # limit 15:0
    .word SEG16_BASE             # base 15:0
    .byte 0x00                   # base 23:16
    .byte 0x9A                   # P=1, DPL=0, S=1, type=code r/x
    .byte 0x00                   # G=0, D=0 (16-bit), limit 19:16 = 0
    .byte 0x00                   # base 31:24
    # 16-bit data: base = SEG16_BASE, limit = 0xFFFF
    .word 0xFFFF                 # limit 15:0
    .word SEG16_BASE             # base 15:0
    .byte 0x00                   # base 23:16
    .byte 0x92                   # P=1, DPL=0, S=1, type=data r/w
    .byte 0x00                   # G=0, D=0 (16-bit), limit 19:16 = 0
    .byte 0x00                   # base 31:24

wakeup_gdt_ptr:
    .word . - wakeup_gdt - 1
    .long WAKEUP_BASE + wakeup_gdt - wakeup_trampoline

wakeup_trampoline_end:
    .code64

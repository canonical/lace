
.global init, gdt, gdtr, _reset

# Segment selectors
.equ SEG_CODE64, 0x08
.equ SEG_DATA64, 0x10
.equ SEG_CODE32, 0x18
.equ SEG_DATA32, 0x20

.text

init:
    # We are still in Real Mode here.
    .code16

    # Load the Global Descriptor Table
    # Note the 32-bit override, because we want to run lgdt with a 32-bit
    # operand size.
    movl $gdtr, %esi
    data32 lgdt %cs:(%si)

    # Enable protected mode in CR0
    movl %cr0, %eax
    orl $0x1, %eax
    movl %eax, %cr0

    # Load the 32-bit code segment selector into CS and jump to Protected Mode
    ljmpl $SEG_CODE32, $1f
1:
    # Now we are in Protected Mode
    .code32

    # Load the 32-bit data segment selector into DS, ES, FS, GS, SS
    mov $SEG_DATA32, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss

    # Clear direction flag for string operations
    cld

    # Copy .data section from flash to RAM
    movl $_data_load, %esi
    movl $_data, %edi
    movl $_data_size, %ecx
    rep movsb

    # Zero initialize .bss section
    movl $_bss, %edi
    movl $_bss_size, %ecx
    xor %eax, %eax
    rep stosb

    # Put the stack below the data segment
    lea _data, %esp

    # Call to Rust code
    call bootblock_main

    cli
1:
    hlt
    jmp 1b

.section .rodata

# Global Descriptor Table
# NOTE: The accessed bit is pre-set in each entry to prevent the CPU from
# trying to write it back on segment load. The GDT lives in ROM so this write
# should fault, but in practice only does on real hardware and hypervisors.
.align 16
gdt:
    .quad 0
    .quad 0x00209B0000000000 # 0x08: 64-bit Code
    .quad 0x0000930000000000 # 0x10: 64-bit Data
    .quad 0x00CF9B000000FFFF # 0x18: 32-bit Code
    .quad 0x00CF93000000FFFF # 0x20: 32-bit Data

# Global Descriptor Table Register
gdtr:
    .word . - gdt - 1
    .long gdt

# CPU reset vector
.section .reset, "ax"
_reset:
    # CPU starts in Real Mode
    .code16

    # Disable interrupts (just in case)
    cli

    # Cache invalidate
    wbinvd

    # Then we jump off lower in memory, because we only have 16 bytes here.
    jmp init

    # Pad to 16 bytes
    .space 16 - (. - _reset), 0x00

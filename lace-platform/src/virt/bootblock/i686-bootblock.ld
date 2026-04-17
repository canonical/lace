ENTRY(_reset)

# Bootblock (top of the firmware flash)
# This will be mapped at 0xffff0000, and the CPU will start excuting
# 16 bytes before that, at 0xfffffff0.
# This will be in Real Mode (with unusual segment base at 0xffff0000)
# and we will only be able to access this 64KB of memory until we switch
# to Protected Mode.

MEMORY
{
    FLASH (rx) : ORIGIN = 0xffff0000, LENGTH = 64K - 16
    RESET (rx) : ORIGIN = 0xfffffff0, LENGTH = 16
    RAM (rwx)  : ORIGIN = 0x01000000, LENGTH = 1M
}

SECTIONS
{
    .text : {
        *(.text*)
    } > FLASH

    .rodata : {
        *(.rodata*)
    } > FLASH

    .eh_frame_hdr : {
        *(.eh_frame_hdr*)
    } > FLASH

    .eh_frame : {
        *(.eh_frame*)
    } > FLASH

    .data : {
        _data = .;
        _data_load = LOADADDR(.data);
        *(.data*)
        _data_size = . - _data;
    } > RAM AT > FLASH

    .bss : {
        _bss = .;
        *(.bss*)
        *(COMMON)
        _bss_size = . - _bss;
        _heap_start = .;
    } > RAM

    .reset : {
        *(.reset*)
    } > RESET
}

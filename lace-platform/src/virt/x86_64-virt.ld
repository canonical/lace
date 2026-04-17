ENTRY(_start)

PHDRS
{
  headers PT_PHDR PHDRS ;
  headers_load PT_LOAD FILEHDR PHDRS ;
  rx PT_LOAD ;
  ro PT_LOAD ;
  rw PT_LOAD ;
}

SECTIONS
{
	/* Firmware ELF lives above the bootblock RAM region (16-17 MiB). */
	_firmware_start = 17M;
	. = _firmware_start + SIZEOF_HEADERS;

	.text ALIGN(4K) : {
		*(.text*)
	} : rx

	.rodata ALIGN(4K) : {
		*(.rodata*)
	} : ro

	.data ALIGN(4K) : {
		*(.data*)
	} : rw

	.bss : {
		*(COMMON)
		*(.bss*)
	} : rw

	_firmware_end = .;
}

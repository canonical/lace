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
	. = 64K + SIZEOF_HEADERS;

	# These things need to be accessed with 16-bit offsets.

	.text16 : {
		*(.text16*)
	} : rx

	# Rest is only needed in 64-bit mode.

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
}

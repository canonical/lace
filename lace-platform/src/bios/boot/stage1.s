    .globl  _start
    .text
    .code16

    # --------------------------------------------------------------------------
    # Stage 1 MBR Bootloader
    #
    # Tasks:
    # 1. Find BIOS Boot Partition.
    # 2. Load Stage 2.
    # 3. Measure Stage 2 (TPM).
    # 4. Jump to Stage 2.
    # --------------------------------------------------------------------------

    .include "boot_defs.s"

    # Constants
    stack_top       = 0x7b00
    buffer_addr     = STAGE2_LOAD_ADDR        # Temp buffer for GPT sectors

    gpt_part_lba    = 0x8048
    gpt_part_count  = 0x8050

_start:
    cli
    xorw    %ax, %ax
    movw    %ax, %ds
    movw    %ax, %es
    movw    %ax, %ss
    movw    $stack_top, %sp
    sti

    movb    %dl, SHARED_DRIVE_NUM

    # Check for INT 13h Extensions
    movb    $0x41, %ah
    movw    $0x55AA, %bx
    int     $0x13
    jc      error_no_lba
    cmpw    $0xAA55, %bx
    jne     error_no_lba
    testb   $1, %cl
    jz      error_no_lba

    # --------------------------------------------------------------------------
    # 1. Find BIOS Boot Partition
    # --------------------------------------------------------------------------

    # Read GPT Header (LBA 1)
    xorl    %eax, %eax
    incw    %ax
    xorl    %edx, %edx
    call    read_gpt_sector
    jc      error_read

    # Get Partition Entry LBA and Count from GPT Header
    # Header is loaded at buffer_addr (0x8000)
    # Partition Entry LBA is at offset 72 (0x48)
    # Number of Partition Entries is at offset 80 (0x50)

    movl    %es:gpt_part_lba, %eax
    movl    %es:gpt_part_lba+4, %edx

    # Store current scan LBA in a local variable (current_scan_lba)
    # We can use the stack or a dedicated memory location.
    # Let's use a dedicated location at the end of the code.
    movl    %eax, current_scan_lba
    movl    %edx, current_scan_lba+4

    movl    %es:gpt_part_count, %ecx

    # Do not scan if there are no partitions
    cmpw    $0, %cx
    je      no_partition_found

scan_loop:
    # Read current partition table sector
    movl    current_scan_lba, %eax
    movl    current_scan_lba+4, %edx
    pushw   %cx             # Save partition count
    call    read_gpt_sector
    popw    %cx             # Restore partition count
    jc      error_read

    # Scan the loaded sector (up to 4 entries per sector)
    movw    $buffer_addr, %si
    movw    $4, %bx         # Maximum 4 entries per 512-byte sector (128 bytes each)

entry_loop:
    pushw   %cx             # Save remaining partition count
    pushw   %si             # Save current entry pointer

    # Compare Type GUID (16 bytes)
    leaw    bios_boot_guid, %di
    movw    $16, %cx
    repe    cmpsb

    popw    %si             # Restore current entry pointer
    popw    %cx             # Restore remaining partition count

    je      found

    # Consume this entry
    decw    %cx
    je      no_partition_found

    # Move to next entry in the buffer
    addw    $128, %si
    decw    %bx
    jnz     entry_loop

    # Move to next sector
    incl    current_scan_lba
    adcl    $0, current_scan_lba+4
    jmp     scan_loop

no_partition_found:

    leaw    msg_no_part, %si
    jmp     error_halt

found:
    # Save Partition Start LBA to SHARED_PART_LBA
    # Entry is at ES:SI
    # First LBA is at offset 32 (0x20)
    movl    %es:0x20(%si), %eax
    movl    %es:0x24(%si), %edx
    movl    %eax, SHARED_PART_LBA
    movl    %edx, SHARED_PART_LBA+4

    # --------------------------------------------------------------------------
    # 2. Load Stage 2
    # --------------------------------------------------------------------------

    # Load Stage 2 from Partition Start LBA
    movl    SHARED_PART_LBA, %eax
    movl    SHARED_PART_LBA+4, %edx
    movw    $STAGE2_SECTOR_COUNT, %cx
    movw    $STAGE2_LOAD_ADDR, %di
    call    bios_read_sectors
    jc      error_read

    # --------------------------------------------------------------------------
    # 3. Measure Stage 2 (TPM)
    # --------------------------------------------------------------------------

    # Measure STAGE2_LOAD_ADDR, length = STAGE2_SECTOR_COUNT * 512

    # Setup for TPM
    # ES:DI = STAGE2_LOAD_ADDR (0x0000:0x8000)
    xorw    %ax, %ax
    movw    %ax, %es
    movw    $STAGE2_LOAD_ADDR, %di

    movl    $STAGE2_SECTOR_COUNT * 512, %ecx
    movl    $0x53544732, %esi   # "STG2"

    call    tpm_measure

    # --------------------------------------------------------------------------
    # 4. Jump to Stage 2
    # --------------------------------------------------------------------------

    # Pass control to Stage 2
    # SHARED_DRIVE_NUM = Drive Number
    # SHARED_PART_LBA = Partition Start LBA
    jmp     STAGE2_LOAD_ADDR

error_read:
    leaw    msg_read_err, %si
    jmp     error_halt
error_no_lba:
    leaw    msg_no_lba, %si
    jmp     error_halt

# Helper: Read GPT Sector (Wrapper)
read_gpt_sector:
    movw    $1, %cx
    movw    $buffer_addr, %di
    jmp     bios_read_sectors

bios_boot_guid: .byte 0x48, 0x61, 0x68, 0x21, 0x49, 0x64, 0x6F, 0x6E, 0x74, 0x4E, 0x65, 0x65, 0x64, 0x45, 0x46, 0x49
current_scan_lba: .quad 0

msg_no_part:    .asciz "No Boot Partition.\r\n"
msg_read_err:   .asciz "Disk Read Error.\r\n"
msg_no_lba:     .asciz "No LBA Support.\r\n"

    .include "boot_utils.s"

# Shared Bootloader Utilities (16-bit Real Mode)

# Function: puts
# Input: DS:SI = Null-terminated string
# Clobbers: AX, BX, SI
puts:
    movb    $0x0E, %ah
    xorw    %bx, %bx
puts_loop:
    lodsb
    orb     %al, %al
    jz      puts_done
    int     $0x10
    jmp     puts_loop
puts_done:
    ret

# Function: error_halt
# Input: DS:SI = Error string
# Does not return
error_halt:
    pushw   %si
    leaw    msg_error_prefix, %si
    call    puts
    popw    %si
    call    puts

    # Print "Press key to reboot..."
    leaw    msg_reboot, %si
    call    puts

    # Wait for keypress
    xorw    %ax, %ax
    int     $0x16

    # Reboot (INT 19h - Bootstrap Loader)
    int     $0x19

    # Fallback if INT 19h returns
    jmp     .

msg_error_prefix: .asciz "Error: "
msg_reboot: .asciz "Press key to reboot...\r\n"

# Function: bios_read_sectors
# Input: EAX:EDX = LBA, CX = Count, ES:DI = Buffer
# Output: CF set on error, AH = Error Code
# Clobbers: SI, BP
bios_read_sectors:
    movw    %sp, %bp

    pushl   %edx
    pushl   %eax
    pushw   %es
    pushw   %di
    pushw   %cx
    pushw   $0x0010
    movw    %sp, %si
    movb    $0x42, %ah
    movb    SHARED_DRIVE_NUM, %dl
    int     $0x13

    movw    %bp, %sp
    ret

# Function: tpm_measure
# Input: ES:DI = Data Buffer, ECX = Length, ESI = Log Data
# Output: None
tpm_measure:
    pusha
    movl    $0x41504354, %ebx   # "TCPA"
    movl    $8, %edx            # PCR 8
    movw    $0xBB07, %ax
    int     $0x1A
    popa
    ret

.ifdef INCLUDE_A20
# Function: enable_a20
# Output: CF set on error
enable_a20:
    # Try BIOS INT 15h, AX=2401h
    movw    $0x2401, %ax
    int     $0x15
    jnc     a20_done

    # Try Fast A20 (Port 0x92)
    inb     $0x92, %al
    testb   $2, %al
    jnz     a20_done
    orb     $2, %al
    andb    $0xFE, %al
    outb    %al, $0x92

a20_done:
    ret
.endif

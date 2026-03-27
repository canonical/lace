    .globl  _start
    .text
    .code16

    # --------------------------------------------------------------------------
    # Stage 2 Bootloader
    #
    # Tasks:
    # 1. Check for Long Mode Support.
    # 2. Enable A20 Line.
    # 3. Load entire ELF file directly to final location (0x10000).
    # 4. Measure entire ELF file into TPM (PCR 8).
    # 5. Verify segments (p_vaddr == Base + p_offset).
    # 6. Zero BSS.
    # 7. Switch to Long Mode.
    # 8. Jump to Core Entry Point.
    # --------------------------------------------------------------------------

    .include "boot_defs.s"

    core_load_seg   = 0x1000        # 0x10000
    core_load_addr  = 0x10000

    # Paging
    pml4_addr       = 0xA000
    pdpt_addr       = 0xB000
    pd_addr         = 0xC000
    efer_msr        = 0xC0000080

_start:
    jmp     entry
    .org    8
core_total_size:
    .long   0       # Size of ELF file in bytes (Patched by installer)

entry:
    # --------------------------------------------------------------------------
    # 1. Check for Long Mode Support
    # --------------------------------------------------------------------------

    # Check for CPU Long Mode Support (CPUID 0x80000001.EDX bit 29)
    movl    $0x80000000, %eax
    cpuid
    cmpl    $0x80000001, %eax
    jb      error_no_long_mode

    movl    $0x80000001, %eax
    cpuid
    bt      $29, %edx
    jnc     error_no_long_mode

    # --------------------------------------------------------------------------
    # 2. Enable A20 Line
    # --------------------------------------------------------------------------

    call    enable_a20

    # --------------------------------------------------------------------------
    # 3. Load Entire ELF File to 0x10000
    # --------------------------------------------------------------------------

    # Calculate sectors needed
    movl    core_total_size, %eax

    # Check if core is too large for Real Mode
    # Conservative Limit: 640KB (0xA0000) - 128KB EBDA = 512KB (0x80000)
    # Available: 0x80000 - 0x10000 (Load Addr) = 0x70000 (448KB)
    cmpl    $0x70000, %eax
    ja      error_too_big

    addl    $511, %eax
    shrl    $9, %eax
    movw    %ax, %cx        # Sector count

    # Target: 0x1000:0000
    movw    $core_load_seg, %bx
    movw    %bx, %es
    xorw    %di, %di

    # LBA = SHARED_PART_LBA + 4 (Stage 2 is 4 sectors)
    movl    SHARED_PART_LBA, %eax
    movl    SHARED_PART_LBA+4, %edx
    addl    $4, %eax
    adcl    $0, %edx

    call    read_sectors_long

    # --------------------------------------------------------------------------
    # 4. Measure Core (TPM)
    # --------------------------------------------------------------------------

    # Measure 0x1000:0000, length = core_total_size

    # Setup for TPM
    movw    $core_load_seg, %bx
    movw    %bx, %es
    xorw    %di, %di

    movl    core_total_size, %ecx
    movl    $0x434F5245, %esi   # "CORE"

    call    tpm_measure

    # Restore ES
    xorw    %ax, %ax
    movw    %ax, %es

    # --------------------------------------------------------------------------
    # 5. Verify Segments & Zero BSS
    # --------------------------------------------------------------------------

    # Point ES:SI to ELF Header at 0x1000:0000
    movw    $core_load_seg, %bx
    movw    %bx, %es
    xorw    %si, %si

    # Check ELF Magic
    cmpl    $0x464C457F, %es:(%si)  # 0x7F 'E' 'L' 'F'
    jne     error_magic

    # Get Program Header Table Offset (e_phoff) at 0x20
    movl    %es:0x20(%si), %ebx     # EBX = PH Offset

    # Get Number of Entries (e_phnum) at 0x38
    movw    %es:0x38(%si), %cx      # CX = PH Count

    # Get Entry Size (e_phentsize) at 0x36
    movw    %es:0x36(%si), %dx      # DX = PH Entry Size

    # Point SI to first PH entry
    # SI = 0 + e_phoff
    # Note: e_phoff is usually small (< 64KB) for typical binaries,
    # but technically it's 64-bit. We assume it fits in 16-bit offset for now
    # or we need to adjust ES if it's large.
    # Since we loaded at 0x1000:0000, and headers are at start,
    # SI = e_phoff is fine if e_phoff < 64KB.
    movw    %bx, %si

verify_loop:
    pushw   %cx             # Save loop counter
    pushw   %si             # Save current PH pointer

    # Check p_type (Offset 0) == PT_LOAD (1)
    cmpl    $1, %es:(%si)
    jne     next_ph

    # Verify: p_vaddr == core_load_addr + p_offset
    # p_vaddr at 0x10
    # p_offset at 0x08

    movl    %es:0x10(%si), %eax     # p_vaddr (low 32)
    movl    %es:0x08(%si), %ebx     # p_offset (low 32)
    addl    $core_load_addr, %ebx   # Expected vaddr

    cmpl    %eax, %ebx
    jne     error_layout            # Mismatch!

    # Zero BSS (p_memsz - p_filesz)
    movl    %es:0x28(%si), %ecx     # p_memsz
    subl    %es:0x20(%si), %ecx     # p_memsz - p_filesz
    jbe     next_ph                 # If memsz <= filesz, no BSS

    # BSS Start = core_load_addr + p_offset + p_filesz
    # (Since p_vaddr == core_load_addr + p_offset)
    # BSS Start = p_vaddr + p_filesz

    movl    %es:0x10(%si), %edi     # p_vaddr
    addl    %es:0x20(%si), %edi     # + p_filesz

    # Zero memory (Linear Address in EDI, Size in ECX)
    call    memzero_linear

next_ph:
    popw    %si
    popw    %cx

    # Advance to next PH entry
    addw    %dx, %si
    decw    %cx
    jnz     verify_loop

    # --------------------------------------------------------------------------
    # 6. Setup Paging
    # --------------------------------------------------------------------------

    # Restore ES to 0
    xorw    %ax, %ax
    movw    %ax, %es

    # Clear Page Tables
    movw    $pml4_addr, %di
    movw    $3072, %cx      # 12KB
    xorl    %eax, %eax
    rep     stosl

    # Link Tables
    movl    $0xB003, %es:pml4_addr  # PML4[0] -> PDPT
    movl    $0xC003, %es:pdpt_addr  # PDPT[0] -> PD

    # Fill PD (1GB Identity)
    movw    $pd_addr, %di
    movl    $0x83, %eax
    movw    $512, %cx
    xorl    %edx, %edx
pd_loop:
    stosl
    xchgl   %eax, %edx
    stosl
    xchgl   %eax, %edx
    addl    $0x200000, %eax
    loop    pd_loop

    # --------------------------------------------------------------------------
    # 7. Switch to Long Mode
    # --------------------------------------------------------------------------

    cli

    # Enable PAE
    movl    %cr4, %eax
    orl     $0x20, %eax
    movl    %eax, %cr4

    # Load CR3
    movl    $pml4_addr, %eax
    movl    %eax, %cr3

    # Enable LME
    movl    $efer_msr, %ecx
    rdmsr
    orl     $0x100, %eax
    wrmsr

    # Load GDT
    lgdt    gdtr

    # Enable PG/PE
    movl    %cr0, %eax
    orl     $0x80000001, %eax
    movl    %eax, %cr0

    # Jump
    ljmp    $0x8, $long_mode

error_magic:
    leaw    msg_magic_err, %si
    jmp     error_halt
error_layout:
    leaw    msg_layout_err, %si
    jmp     error_halt
error_read:
    leaw    msg_read_err, %si
    jmp     error_halt
error_too_big:
    leaw    msg_too_big, %si
    jmp     error_halt
error_no_long_mode:
    leaw    msg_no_long_mode, %si
    jmp     error_halt

# Helper: Read Sectors Long (Handles > 64KB / Segment Crossing)
# Input: EAX:EDX = LBA, CX = Count, ES:DI = Buffer
read_sectors_long:
    pusha
read_loop:
    # Max sectors per read = 32 (16KB) to align with segment increments
    cmpw    $0, %cx
    je      read_done

    movw    %cx, %bx
    cmpw    $32, %bx
    jbe     1f
    movw    $32, %bx
1:
    # Read BX sectors
    pushw   %bx     # Save chunk count

    # Call BIOS (Shared)
    pushl   %edx            # Save LBA (High)
    pushl   %eax            # Save LBA (Low)
    pushw   %cx             # Save total loop counter

    movw    %bx, %cx        # Set count for bios_read_sectors
    call    bios_read_sectors

    popw    %cx             # Restore total loop counter
    popl    %eax            # Restore LBA (Low)
    popl    %edx            # Restore LBA (High)

    jc      error_read

    popw    %bx     # Restore chunk count

    # Advance LBA
    movzx   %bx, %esi
    addl    %esi, %eax
    adcl    $0, %edx

    # Decrement Total Count
    subw    %bx, %cx

    # Advance Buffer (ES)
    # 32 sectors * 512 bytes = 16384 bytes
    # 16384 / 16 = 1024 (0x400) paragraphs
    shlw    $5, %bx         # Sectors * 32 = Paragraphs
    movw    %es, %si
    addw    %bx, %si
    movw    %si, %es

    jmp     read_loop

read_done:
    popa
    ret

# Helper: Memzero Linear (EDI=Dest, ECX=Size)
memzero_linear:
    pusha
zero_loop:
    cmpl    $0, %ecx
    je      zero_done

    # Setup ES for Dest
    movl    %edi, %ebx
    shrl    $4, %ebx
    movw    %bx, %es
    movl    %edi, %ebx
    andw    $0xF, %bx

    movb    $0, %al
    movb    %al, %es:(%bx)

    incl    %edi
    decl    %ecx
    jmp     zero_loop
zero_done:
    popa
    ret

    .code64
long_mode:
    movw    $0x10, %ax
    movw    %ax, %ds
    movw    %ax, %es
    movw    %ax, %fs
    movw    %ax, %gs
    movw    %ax, %ss

    # --------------------------------------------------------------------------
    # 8. Jump to Core Entry Point
    # --------------------------------------------------------------------------
    # CPU:   64-bit Long Mode, Paging Enabled (Identity 0-1GB), Interrupts Disabled
    # CS:    0x08 (64-bit Code Segment)
    # DS/ES/FS/GS: 0x10 (64-bit Data Segment)
    # SS:    0x10 (64-bit Data Segment)
    # RSP:   Undefined
    # Mem:   Core loaded at 0x10000
    # Data:  Shared Data at 0x7B00 (Drive Num, Partition LBA)

    # Jump to Entry Point
    # We need to read e_entry from the ELF header at 0x10000
    movq    $0x10000, %rbx
    movq    0x18(%rbx), %rax    # e_entry
    jmp     *%rax

gdt:
    .quad 0
    .quad 0x00209A0000000000
    .quad 0x0000920000000000
gdtr:
    .word . - gdt - 1
    .long gdt

    .code16

msg_magic_err:  .asciz "Invalid ELF Magic.\r\n"
msg_layout_err: .asciz "Segment Layout Mismatch.\r\n"
msg_read_err:   .asciz "Disk Read Failed.\r\n"
msg_too_big:    .asciz "Core exceeds low memory limit.\r\n"
msg_no_long_mode: .asciz "CPU does not support Long Mode.\r\n"

    .equ INCLUDE_A20, 1
    .include "boot_utils.s"

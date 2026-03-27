# Shared Bootloader Definitions

# Memory Map
# 0x0500 - 0x7B00: Stack (Stage 1 & 2)
# 0x7B00:          Shared Data Area
# 0x7C00:          Stage 1 Code
# 0x8000:          Stage 2 Code / Buffer
# 0xA000:          PML4
# 0xB000:          PDPT
# 0xC000:          PD
# 0x10000:         Core Load Address

# Shared Data Variables (at 0x7B00)
.equ SHARED_DRIVE_NUM,      0x7B00  # 1 byte
.equ SHARED_PART_LBA,       0x7B08  # 8 bytes

# Stage 2 Constants
.equ STAGE2_LOAD_ADDR,      0x8000
.equ STAGE2_SECTOR_COUNT,   4       # 2KB

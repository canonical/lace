// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only

//! Linux boot parameters definitions
//! The canonical source for these is `arch/x86/include/uapi/asm/bootparam.h`
//! in the Linux kernel source code.

use crate::e820::E820Entry;

/// Maximum number of E820 entries in the Linux boot parameters
pub const E820_MAX_ENTRIES_ZEROPAGE: usize = 128;

/// Linux boot parameters structure
#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
pub struct BootParams {
    pub screen_info: ScreenInfo,
    pub apm_bios_info: [u8; 0x14],
    pub _pad2: [u8; 4],
    pub tboot_addr: u64,
    pub ist_info: [u8; 0x10],
    pub acpi_rsdp_addr: u64,
    pub _pad3: [u8; 8],
    pub hd0_info: [u8; 16],
    pub hd1_info: [u8; 16],
    pub sys_desc_table: [u8; 0x10],
    pub olpc_ofw_header: [u8; 0x10],
    pub ext_ramdisk_image: u32,
    pub ext_ramdisk_size: u32,
    pub ext_cmd_line_ptr: u32,
    pub _pad4: [u8; 112],
    pub cc_blob_address: u32,
    pub edid_info: [u8; 128],
    pub efi_info: EfiInfo,
    pub alt_mem_k: u32,
    pub scratch: u32,
    pub e820_entries: u8,
    pub eddbuf_entries: u8,
    pub edd_mbr_sig_buf_entries: u8,
    pub kbd_status: u8,
    pub secure_boot: u8,
    pub _pad5: [u8; 2],
    pub sentinel: u8,
    pub _pad6: [u8; 1],
    pub hdr: SetupHeader,
    pub _pad7: [u8; 0x290 - 0x1f1 - core::mem::size_of::<SetupHeader>()],
    pub edd_mbr_sig_buffer: [u32; 16],
    pub e820_table: [E820Entry; E820_MAX_ENTRIES_ZEROPAGE],
    pub _pad8: [u8; 48],
    pub eddbuf: [u8; 496], // sizeof (struct edd_info) * 6
    pub _pad9: [u8; 276],
}

impl Default for BootParams {
    fn default() -> Self {
        unsafe {
            // SAFETY: C struct can be zero-initialized
            core::mem::zeroed()
        }
    }
}

// orig_video_is_vga
pub const VIDEO_TYPE_MDA: u32 = 0x10;
pub const VIDEO_TYPE_CGA: u32 = 0x11;
pub const VIDEO_TYPE_EGAM: u32 = 0x20;
pub const VIDEO_TYPE_EGAC: u32 = 0x21;
pub const VIDEO_TYPE_VGAC: u32 = 0x22;
pub const VIDEO_TYPE_VLFB: u32 = 0x23;
pub const VIDEO_TYPE_PICA_S3: u32 = 0x30;
pub const VIDEO_TYPE_MIPS_G364: u32 = 0x31;
pub const VIDEO_TYPE_SGI: u32 = 0x33;
pub const VIDEO_TYPE_TGAC: u32 = 0x40;
pub const VIDEO_TYPE_SUN: u32 = 0x50;
pub const VIDEO_TYPE_SUNPCI: u32 = 0x51;
pub const VIDEO_TYPE_PMAC: u32 = 0x60;
pub const VIDEO_TYPE_EFI: u32 = 0x70;
pub const VIDEO_FLAGS_NOCURSOR: u32 = 1 << 0;
pub const VIDEO_CAPABILITY_SKIP_QUIRKS: u32 = 1 << 0;
pub const VIDEO_CAPABILITY_64BIT_BASE: u32 = 1 << 1;

/// Linux screen information structure
#[derive(Clone, Copy, Debug, Default)]
#[repr(C, packed)]
pub struct ScreenInfo {
    pub orig_x: u8,
    pub orig_y: u8,
    pub ext_mem_k: u16,
    pub orig_video_page: u16,
    pub orig_video_mode: u8,
    pub orig_video_cols: u8,
    pub flags: u8,
    pub unused2: u8,
    pub orig_video_ega_bx: u16,
    pub unused3: u16,
    pub orig_video_lines: u8,
    pub orig_video_is_vga: u8,
    pub orig_video_points: u16,
    pub lfb_width: u16,
    pub lfb_height: u16,
    pub lfb_depth: u16,
    pub lfb_base: u32,
    pub lfb_size: u32,
    pub cl_magic: u16,
    pub cl_offset: u16,
    pub lfb_linelength: u16,
    pub red_size: u8,
    pub red_pos: u8,
    pub green_size: u8,
    pub green_pos: u8,
    pub blue_size: u8,
    pub blue_pos: u8,
    pub rsvd_size: u8,
    pub rsvd_pos: u8,
    pub vesapm_seg: u16,
    pub vesapm_off: u16,
    pub pages: u16,
    pub vesa_attributes: u16,
    pub capabilities: u32,
    pub ext_lfb_base: u32,
    pub _reserved: [u8; 2],
}

/// Linux EFI information structure
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct EfiInfo {
    pub efi_loader_signature: u32,
    pub efi_systab: u32,
    pub efi_memdesc_size: u32,
    pub efi_memdesc_version: u32,
    pub efi_memmap: u32,
    pub efi_memmap_size: u32,
    pub efi_systab_hi: u32,
    pub efi_memmap_hi: u32,
}

// ram_size
pub const RAMDISK_IMAGE_START_MASK: u16 = 0x07FF;
pub const RAMDISK_PROMPT_FLAG: u16 = 0x8000;
pub const RAMDISK_LOAD_FLAG: u16 = 0x4000;

// loadflags
pub const LOADED_HIGH: u32 = 1 << 0;
pub const KASLR_FLAG: u32 = 1 << 1;
pub const QUIET_FLAG: u32 = 1 << 5;
pub const KEEP_SEGMENTS: u32 = 1 << 6;
pub const CAN_USE_HEAP: u32 = 1 << 7;

// xloadflags
pub const XLF_KERNEL_64: u32 = 1 << 0;
pub const XLF_CAN_BE_LOADED_ABOVE_4G: u32 = 1 << 1;
pub const XLF_EFI_HANDOVER_32: u32 = 1 << 2;
pub const XLF_EFI_HANDOVER_64: u32 = 1 << 3;
pub const XLF_EFI_KEXEC: u32 = 1 << 4;
pub const XLF_5LEVEL: u32 = 1 << 5;
pub const XLF_5LEVEL_ENABLED: u32 = 1 << 6;
pub const XLF_MEM_ENCRYPTION: u32 = 1 << 7;

// subarch
pub const X86_SUBARCH_PC: u32 = 0;
pub const X86_SUBARCH_LGUEST: u32 = 1;
pub const X86_SUBARCH_XEN: u32 = 2;
pub const X86_SUBARCH_INTEL_MID: u32 = 3;
pub const X86_SUBARCH_CE4100: u32 = 4;
pub const X86_NR_SUBARCHS: u32 = 5;

#[derive(Clone, Copy, Debug, Default)]
#[repr(C, packed)]
pub struct SetupHeader {
    pub setup_sects: u8,
    pub root_flags: u16,
    pub syssize: u32,
    pub ram_size: u16,
    pub vid_mode: u16,
    pub root_dev: u16,
    pub boot_flag: u16,
    pub jump: u16,
    pub header: u32,
    pub version: u16,
    pub realmode_swtch: u32,
    pub start_sys_seg: u16,
    pub kernel_version: u16,
    pub type_of_loader: u8,
    pub loadflags: u8,
    pub setup_move_size: u16,
    pub code32_start: u32,
    pub ramdisk_image: u32,
    pub ramdisk_size: u32,
    pub bootsect_kludge: u32,
    pub heap_end_ptr: u16,
    pub ext_loader_ver: u8,
    pub ext_loader_type: u8,
    pub cmd_line_ptr: u32,
    pub initrd_addr_max: u32,
    pub kernel_alignment: u32,
    pub relocatable_kernel: u8,
    pub min_alignment: u8,
    pub xloadflags: u16,
    pub cmdline_size: u32,
    pub hardware_subarch: u32,
    pub hardware_subarch_data: u64,
    pub payload_offset: u32,
    pub payload_length: u32,
    pub setup_data: u64,
    pub pref_address: u64,
    pub init_size: u32,
    pub handover_offset: u32,
    pub kernel_info_offset: u32,
}

// setup_data/setup_indirect types
pub const SETUP_NONE: u32 = 0;
pub const SETUP_E820_EXT: u32 = 1;
pub const SETUP_DTB: u32 = 2;
pub const SETUP_PCI: u32 = 3;
pub const SETUP_EFI: u32 = 4;
pub const SETUP_APPLE_PROPERTIES: u32 = 5;
pub const SETUP_JAILHOUSE: u32 = 6;
pub const SETUP_CC_BLOB: u32 = 7;
pub const SETUP_IMA: u32 = 8;
pub const SETUP_RNG_SEED: u32 = 9;
pub const SETUP_KEXEC_KHO: u32 = 10;
pub const SETUP_ENUM_MAX: u32 = SETUP_KEXEC_KHO;

pub const SETUP_INDIRECT: u32 = 1 << 31;
pub const SETUP_TYPE_MAX: u32 = SETUP_INDIRECT;

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct SetupData {
    pub next: u64,
    pub type_: u32,
    pub len: u32,
    pub data: [u8; 0],
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct SetupIndirect {
    pub type_: u32,
    pub reserved: u32,
    pub len: u64,
    pub addr: u64,
}


// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Tool for building stubble images",
    long_about = "pewrap combines a pre-built stub binary with additional binaries \
                  (Linux kernel, initrd, device trees), SBAT data, and HWIDs \
                  to generate a stubble image.\n\n\
                  The tool takes a stub PE binary and adds new sections containing the \
                  specified payloads. This enables creation of stubble images that \
                  can be loaded by UEFI firmware with all necessary components embedded."
)]
pub struct Args {
    /// Path to input stub PE image
    ///
    /// The stub PE must be a valid UEFI executable that will serve as the base
    /// for the output image. All sections from the stub will be preserved in
    /// the output, with new sections appended.
    #[arg(short, long, value_name = "FILE")]
    pub stub: PathBuf,

    /// Path to output stubble image
    ///
    /// The resulting stubble image will contain all sections from the stub plus
    /// any additional sections specified by other arguments. Section offsets
    /// and headers are automatically recalculated to maintain PE format validity.
    #[arg(short, long, value_name = "FILE")]
    pub output: PathBuf,

    /// Path to linux kernel image to add
    ///
    /// Embeds the kernel image in a .linux section for boot loading.
    /// The kernel must be a PE binary. For Linux kernels, this typically
    /// means a PE-wrapped bzImage or similar format suitable for UEFI loading.
    #[arg(short, long, value_name = "FILE")]
    pub linux: Option<PathBuf>,

    /// Path to initrd image to add
    ///
    /// Embeds the initial ramdisk in a .initrd section for boot loading.
    /// The initrd is loaded into memory by the bootloader before starting the
    /// kernel, providing an early userspace environment and drivers.
    #[arg(short, long, value_name = "FILE")]
    pub initrd: Option<PathBuf>,

    /// Kernel command line to add
    ///
    /// Embeds the command line string in a .cmdline section. This specifies
    /// boot parameters and configuration options passed to the kernel at boot time,
    /// such as root device, console settings, and kernel features.
    #[arg(short, long, value_name = "STRING")]
    pub cmdline: Option<String>,

    /// Add .sbat section with SBAT data from the given file
    ///
    /// SBAT (UEFI Secure Boot Advanced Targeting) metadata for revocation support.
    /// This section contains versioning information used by UEFI Secure Boot to
    /// enable fine-grained revocation of vulnerable bootloader components without
    /// requiring full certificate revocation.
    #[arg(long, value_name = "FILE")]
    pub sbat: Option<PathBuf>,

    /// HWIDs directory to scan for JSON files
    ///
    /// Scans the directory recursively for HWID JSON files and generates a
    /// .hwids section containing CHID-to-compatible mappings. These mappings
    /// are used for automatic device tree selection based on hardware
    /// identification (SMBIOS/EDID), allowing a single stubble image to support
    /// multiple hardware platforms.
    #[arg(long, value_name = "DIR")]
    pub hwids: Option<PathBuf>,

    /// Device tree blobs to add for automatic loading
    ///
    /// Each DTB is embedded in a separate .dtbauto section for automatic loading
    /// based on hardware detection. Multiple DTBs can be specified by repeating
    /// this argument. The stubble image uses HWID mappings to select the appropriate
    /// DTB for the current hardware platform at boot time.
    #[arg(long, value_name = "FILE")]
    pub dtbauto: Vec<PathBuf>,

    /// Post-process a lace-stubble PE binary for systemd-ukify compatibility
    ///
    /// systemd-ukify works with lace-stubble, but unlike pewrap, systemd-ukify does not
    /// have the ability to relayout the raw layout of a PE image, and as a result the
    /// maximum number of sections it can handle is limited to how many fit before the
    /// original SizeOfHeaders value.
    ///
    /// pewrap can relayout the raw layout of the image, which gives it the ability to
    /// add as many sections as able to fit before the VirtualAddress of the first section.
    ///
    /// This option instructs pewrap to relayout the image by setting SizeOfHeaders
    /// to as large as possible, maximizing the number of sections that can be added by
    /// systemd-ukify later on.
    ///
    /// Additionally it sets the PE major version to 1, which is also required by ukify,
    /// otherwise it will never indicate support for LoadFile2.
    #[arg(long)]
    pub post_process_for_ukify: bool,
}

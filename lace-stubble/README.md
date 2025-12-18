# lace-stubble

`lace-stubble` is a small, (mostly) memory-safe stub bootloader for running the Linux kernel on UEFI, implemented using the libraries provided by Project Lace.

It is a Rust implementation of the earlier [stubble](https://github.com/ubuntu/stubble) project, which in turn is a minimal derivative of systemd-stub with fewer features.

## Overview

`lace-stubble` is a "stub" EFI binary that can be specialized into a bootable image by embedding resources (kernel, initrd, etc.) into PE sections.

## Features

- Loading of Linux kernel, initramfs, command line, and device trees from a single executable.
- Automatic Device Tree Selection:
  - Determines the platform's compatible string by inspecting the firmware-provided DTB or by matching Computer Hardware IDs (CHIDs) against an embedded `.hwids` database.
  - Selects and installs the correct Device Tree Blob (DTB) from embedded `.dtbauto` sections based on the platform compatible string.
- Command Line Handling: Supports embedded command lines via the `.cmdline` section, with fallback to UEFI Load Options.
- Memory Safety: Written in Rust, leveraging the `uefi` crate and `lace-util` for safe parsing and execution.

## PE Sections

`lace-stubble` looks for the following sections in its own PE image:

| Section Name | Description | Requirement |
|--------------|-------------|-------------|
| `.linux`     | The Linux kernel image. | **Mandatory** |
| `.initrd`    | The initial RAM disk (initramfs). | Optional** |
| `.cmdline`   | Kernel command line arguments. | Optional* |
| `.hwids`     | JSON/Binary mapping of CHIDs to compatible strings. | Optional |
| `.dtbauto`   | Device Tree Blob (DTB). Multiple sections allowed. | Optional |

\* If `.cmdline` is missing, `lace-stubble` will attempt to use the UEFI Load Options (arguments passed to the EFI binary).

\** If `.initrd` is missing, an external initrd will be used if available (via UEFI Load File2 Protocol).

## Build

`lace-stubble` is part of the Lace workspace. To build it:

```none
cargo build -p lace-stubble --target MY_TARGET [--release]
```

Note that `lace-stubble` targets `x86_64-unknown-uefi` or `aarch64-unknown-uefi`. Ensure you have the appropriate target installed:

```none
rustup target add x86_64-unknown-uefi
# or
rustup target add aarch64-unknown-uefi
```

## Usage

To create a bootable `lace-stubble` image, you use the `pewrap` tool (part of Project Lace) to inject the necessary sections into the base `stubble.efi` binary.

### Example with `pewrap`

```none
# Wrap resources into the stub
cargo run -p pewrap -- \
    --stub target/x86_64-unknown-uefi/release/lace-stubble.efi \
    --output bootable.efi \
    --linux /boot/vmlinuz-linux \
    --initrd /boot/initramfs-linux.img \
    --cmdline "console=ttyS0 console=tty0 root=UUID=... rw" \
    --hwids data/hwids/json \
    --dtbauto path/to/board1.dtb \
    --dtbauto path/to/board2.dtb
```

This command creates `bootable.efi`, which contains the kernel, initrd, command line, and DTBs. When executed on a UEFI system, `lace-stubble` will automatically select the correct DTB for the hardware and boot the kernel.

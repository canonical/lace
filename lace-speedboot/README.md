# lace-speedboot

A fast UEFI boot menu that automatically discovers and boots Linux systems using GRUB configurations.

## Overview

lace-speedboot is a UEFI application that:

1. Scans all available block devices for GRUB configuration files
2. Parses menu entries from `grub.cfg` files
3. Displays a simple boot menu to the user
4. Boots the selected Linux kernel with its initrd and command line

## Features

- **Automatic Discovery**: Finds GRUB configurations in common locations across all filesystems
- **GRUB Compatibility**: Parses standard GRUB menu entries including submenus
- **Fast Boot**: Minimal overhead compared to traditional GRUB
- **Simple Interface**: Clean text-based menu for boot entry selection

## Supported Filesystems

- FAT (via UEFI Simple Filesystem Protocol)
- Any filesystem supported by UEFI firmware

## Common GRUB Locations

lace-speedboot searches for grub.cfg in these locations:

- `boot/grub/grub.cfg`
- `grub/grub.cfg`
- `boot/grub2/grub.cfg`
- `grub2/grub.cfg`
- `EFI/ubuntu/grub.cfg`
- `EFI/debian/grub.cfg`
- `EFI/fedora/grub.cfg`

## Building

```bash
cargo build -p lace-speedboot --target x86_64-unknown-uefi
```

The resulting `.efi` file will be in `target/x86_64-unknown-uefi/debug/lace-speedboot.efi`.

## Testing

A test script is provided to test lace-speedboot with an Ubuntu cloud image:

```bash
./scripts/vm_manage.py create
./scripts/vm_manage.py start --app speedboot
```

This will:
1. Download an Ubuntu cloud image
2. Create a test disk image
3. Replace the UEFI bootloader with lace-speedboot
4. Boot the system in QEMU

## Usage

To use lace-speedboot, replace your system's EFI bootloader (`bootx64.efi`) with the lace-speedboot binary. On boot, it will:

1. Scan all disks for GRUB configurations
2. Display found menu entries
3. Wait for user input to select a boot entry
4. Boot the selected Linux system

## Architecture

lace-speedboot uses:
- `lace-platform::efi::fs` for filesystem access
- `lace-util::grub` for GRUB configuration parsing
- `lace-platform::linux` for booting Linux kernels

## License

Dual licensed under GPL-2.0-only OR GPL-3.0-only.

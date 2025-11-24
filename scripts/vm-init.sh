#!/bin/bash -e

VM_DIR="$(dirname $0)/../vm"

if [ -e "$VM_DIR" ]; then
	echo "error: VM was already initialized" >&2
	exit 1
fi

mkdir "$VM_DIR"
cd "$VM_DIR"

# Get firmware
cp /usr/share/OVMF/OVMF_CODE_4M.fd code.fd
cp /usr/share/OVMF/OVMF_VARS_4M.fd vars.fd

# Create disk
qemu-img create -f raw hdd.img 1G
sgdisk -n 1:0:+100MiB hdd.img
mformat \
	-i hdd.img@@1048576 \
	-T 204800 \
	-F

# Create ESP \\EFI\BOOT directory
mmd -i hdd.img@@1048576 /EFI
mmd -i hdd.img@@1048576 /EFI/BOOT

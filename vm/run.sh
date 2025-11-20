#!/bin/bash -e

cd $(dirname $0)

pushd ..
cargo build -p lace-stubble --target x86_64-unknown-uefi
cargo run -p pewrap -- \
	--stub target/x86_64-unknown-uefi/debug/lace-stubble.efi \
	--output vm/stubble.efi \
	--linux /vmlinuz
popd

mcopy \
	-o \
	-i hdd.img@@1048576 \
	stubble.efi \
	::/EFI/BOOT/BOOTX64.EFI

qemu-system-x86_64 \
	-M q35,accel=kvm \
	-m 1G \
	-nographic \
	-drive if=pflash,unit=0,format=raw,file=code.fd,readonly=on \
	-drive if=pflash,unit=1,format=raw,file=vars.fd \
	-drive if=virtio,format=raw,file=hdd.img

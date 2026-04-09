#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only

"""Lace test VM management script"""

import argparse
import copy
import guestfs
import json
import os
import pefile
import platform
import requests
import shutil
import struct
import subprocess
import time
import uuid

# Enable DEBUG mode
DEBUG = False

# Use local image server for Ubuntu cloud images
LOCAL_IMG_SERVER = False

# Ubuntu release to use for testing
UBUNTU_RELEASE = "resolute"

# Constants for size units
KIB = 1024
MIB = 1024 * KIB
GIB = 1024 * MIB

# Disk sector size
SECTOR_SIZE = 512

# Default VM configurations for different architectures
VM_DEFAULTS = {
    "x86_64": {
        "arch": "x86_64",
        "machine": "q35",
        "cpu": {
            "model": "qemu64",
        },
        "fw": {
            "dir": "/usr/share/OVMF",
            "code": "OVMF_CODE_4M.fd",
            "vars": "OVMF_VARS_4M.fd",
        },
    },
    "aarch64": {
        "arch": "aarch64",
        "machine": "virt",
        "cpu": {
            "model": "cortex-a57",
        },
        "fw": {
            "dir": "/usr/share/AAVMF",
            "code": "AAVMF_CODE.secboot.fd",
            "vars": "AAVMF_VARS.fd",
        },
    },
}

# EFI system partition type GUID
EFI_SYSTEM_PARTITION_TYPE_GUID = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"
# BIOS Boot Partition type GUID
BIOS_BOOT_PARTITION_TYPE_GUID = "21686148-6449-6E6F-744E-656564454649"

# Cloud-init user-data template
CI_USER_DATA = """#cloud-config
password: ubuntu
chpasswd: { expire: False }
ssh_pwauth: True
"""

# EFI suffixes for different architectures
EFI_SUFFIXES = {
    "x86_64": "X64.EFI",
    "aarch64": "AA64.EFI",
}


class GPTPartition:
    def __init__(self, type_guid, unique_guid, first_lba, last_lba, attributes, name):
        self.type_guid = type_guid
        self.unique_guid = unique_guid
        self.first_lba = first_lba
        self.last_lba = last_lba
        self.attributes = attributes
        self.name = name

    @classmethod
    def decode(cls, entry_bytes):
        if len(entry_bytes) < 128:
            raise ValueError("Partition entry must be at least 128 bytes")

        type_guid_bytes = entry_bytes[:16]
        if type_guid_bytes == b'\x00' * 16:
            return None

        unique_guid_bytes = entry_bytes[16:32]
        first_lba = struct.unpack_from("<Q", entry_bytes, 32)[0]
        last_lba = struct.unpack_from("<Q", entry_bytes, 40)[0]
        attributes = struct.unpack_from("<Q", entry_bytes, 48)[0]
        name_bytes = entry_bytes[56:128]

        # Decode name (UTF-16LE, null-terminated)
        name = name_bytes.decode('utf-16-le').split('\x00')[0]

        type_guid = uuid.UUID(bytes_le=type_guid_bytes)
        unique_guid = uuid.UUID(bytes_le=unique_guid_bytes)

        return cls(type_guid, unique_guid, first_lba, last_lba, attributes, name)

    def __repr__(self):
        return f"<GPTPartition type={self.type_guid} start={self.first_lba} end={self.last_lba} name='{self.name}'>"


class GPT:
    def __init__(self, file_obj):
        self.file_obj = file_obj
        self.partitions = []
        self.sector_size = 512

    def read(self):
        self.partitions = []
        # Read GPT Header (LBA 1)
        self.file_obj.seek(self.sector_size)
        header = self.file_obj.read(self.sector_size)

        if len(header) < self.sector_size or header[:8] != b"EFI PART":
            raise ValueError("Invalid GPT signature")

        # Parse Header
        part_entry_lba = struct.unpack_from("<Q", header, 72)[0]
        num_part_entries = struct.unpack_from("<I", header, 80)[0]
        part_entry_size = struct.unpack_from("<I", header, 84)[0]

        # Read Partition Entries
        self.file_obj.seek(part_entry_lba * self.sector_size)
        entries_data = self.file_obj.read(num_part_entries * part_entry_size)

        for i in range(num_part_entries):
            entry_offset = i * part_entry_size
            entry = entries_data[entry_offset : entry_offset + part_entry_size]

            part = GPTPartition.decode(entry)
            if part:
                self.partitions.append(part)

    def find_partition_by_type(self, type_guid_str):
        target_uuid = uuid.UUID(type_guid_str)
        for part in self.partitions:
            if part.type_guid == target_uuid:
                return part
        return None


def default_vm_dir():
    """
    Returns the default directory for VM files, which is one level up
    from the "scripts" directory and is called "vm".
    """
    return os.path.join(
        os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "vm"
    )


def parse_disk_size(disk_size):
    """
    Parse a disk size string with optional unit suffix (K, M, G)
    and return the size in bytes
    """
    units = {"K": KIB, "M": MIB, "G": GIB}
    unit = 1
    if disk_size[-1] in units:
        unit = units[disk_size[-1]]
        disk_size = disk_size[:-1]
    return int(disk_size) * unit


def ubuntu_cloud_url(release, arch):
    """
    Construct the URL for the Ubuntu cloud image for the given release and architecture
    """
    if arch == "x86_64":
        arch = "amd64"
    elif arch == "aarch64":
        arch = "arm64"
    else:
        raise ValueError(f"Unsupported architecture for Ubuntu cloud image: {arch}")

    if LOCAL_IMG_SERVER:
        return f"http://localhost/cloudimg/{release}-server-cloudimg-{arch}.img"

    return f"http://cloud-images.ubuntu.com/{release}/current/{release}-server-cloudimg-{arch}.img"


def download_file(url, dest_path):
    """
    Download a file from the given URL to the specified destination path
    """
    resp = requests.get(url, timeout=10, stream=True)
    if resp.status_code != 200:
        raise RuntimeError(
            f"Failed to download file from {url}: HTTP {resp.status_code}"
        )
    with open(dest_path, "wb") as file:
        for chunk in resp.iter_content(chunk_size=4096):
            file.write(chunk)


def best_vm_accel(vm_arch):
    """
    Determine the best VM accelerator to use based on the host and VM architecture.
    """
    if platform.machine() == vm_arch:
        return "kvm"  # Use hardware acceleration if host and target arch match
    return "tcg"  # Use software emulation if host and target arch differ


def create_disk_image(args):
    """Create a disk image for the VM based on an Ubuntu cloud image"""

    # Download Ubuntu cloud image
    ubuntu_img_url = ubuntu_cloud_url(UBUNTU_RELEASE, args.arch)
    ubuntu_img_path = os.path.join(args.dir, "ubuntu-cloud.img")
    print(f"Downloading Ubuntu cloud image from {ubuntu_img_url}...")
    download_file(ubuntu_img_url, ubuntu_img_path)
    print("Download complete.")

    disk_image_path = os.path.join(args.dir, "disk.img")
    gfs = guestfs.GuestFS(python_return_dict=True)
    if DEBUG:
        gfs.set_trace(1)
    gfs.disk_create(disk_image_path, "raw", args.disk_size)
    gfs.add_drive_opts(disk_image_path, format="raw", readonly=0)
    gfs.add_drive_opts(ubuntu_img_path, format="qcow2", readonly=1)
    gfs.launch()

    # Find Ubuntu OS root
    roots = gfs.inspect_os()
    if len(roots) == 0:
        raise RuntimeError("No operating systems found in the Ubuntu cloud image")

    # Mount Ubuntu filesystems
    mps = gfs.inspect_get_mountpoints(roots[0])
    # NOTE: /dev/sda is going to be ignored here, this is a tmpfs,
    # libguestfs just needs a block device here `none` doesn't work
    gfs.mount_vfs("size=1M", "tmpfs", "/dev/sda", "/")
    gfs.mkdir_p("/cloudimg")
    for mount_point, device in sorted(mps.items(), key=lambda k: len(k[0])):
        gfs.mount_ro(device, f"/cloudimg{mount_point}")

    # Create GPT partition table and partitions
    device = gfs.list_devices()[0]
    gfs.part_init(device, "gpt")

    current_sector = 2048

    # Add BIOS Boot Partition for x86_64
    if args.arch == "x86_64":
        bios_boot_size_sectors = 1 * MIB // SECTOR_SIZE  # 1MB
        gfs.part_add(
            device, "p", current_sector, current_sector + bios_boot_size_sectors - 1
        )
        gfs.part_set_gpt_type(device, 1, BIOS_BOOT_PARTITION_TYPE_GUID)
        current_sector += bios_boot_size_sectors

    # ESP
    esp_size_sectors = 512 * MIB // SECTOR_SIZE
    gfs.part_add(device, "p", current_sector, current_sector + esp_size_sectors - 1)
    # Note: Partition index depends on whether we added BIOS boot partition
    esp_part_idx = 2 if args.arch == "x86_64" else 1
    gfs.part_set_gpt_type(device, esp_part_idx, EFI_SYSTEM_PARTITION_TYPE_GUID)
    current_sector += esp_size_sectors

    # Root partition
    gfs.part_add(device, "p", current_sector, -2048)

    # Format filesystems
    partitions = list(filter(lambda s: s.startswith(device), gfs.list_partitions()))
    if args.arch == "x86_64":
        esp_partition = partitions[1]
        root_partition = partitions[2]
    else:
        esp_partition = partitions[0]
        root_partition = partitions[1]

    gfs.mkfs("vfat", esp_partition)
    gfs.mkfs(args.root_fs_type, root_partition)
    gfs.set_label(root_partition, "cloudimg-rootfs")

    # Copy files from Ubuntu cloud image to new disk
    gfs.mkdir_p("/disk")
    gfs.mount(root_partition, "/disk")
    gfs.mkdir_p("/disk/boot/efi")
    gfs.mount(esp_partition, "/disk/boot/efi")
    gfs.cp_a("/cloudimg/.", "/disk/")

    # Write new fstab
    fstab_content = f"""# /etc/fstab: static file system information.
UUID={gfs.vfs_uuid(root_partition)} / {args.root_fs_type} errors=remount-ro 0 1
UUID={gfs.vfs_uuid(esp_partition)} /boot/efi vfat umask=0077 0 2
"""
    gfs.write("/disk/etc/fstab", fstab_content)

    # Remove EFI files
    gfs.rm_rf("/disk/boot/efi/EFI/")

    # Close disks
    gfs.umount_all()
    gfs.shutdown()
    gfs.close()

    # Delete cloud image
    os.remove(ubuntu_img_path)


def do_init(args):
    """Handler for the 'init' command"""

    # Create VM directory if it doesn't exist
    # Otherwise, raise an error
    os.makedirs(args.dir, exist_ok=False)

    # Get firmware paths
    defaults = VM_DEFAULTS.get(args.arch)
    if not defaults:
        raise ValueError(f"Unsupported architecture: {args.arch}")

    # Save VM configuration
    config = copy.deepcopy(defaults)
    config["cpu"]["cores"] = args.cores
    config["ram"] = args.ram
    config["disk"] = {
        "format": "raw",
        "file": "disk.img",
    }
    with open(
        os.path.join(args.dir, "config.json"), "w", encoding="utf-8"
    ) as config_file:
        json.dump(config, config_file, indent=4)

    # Copy firmware files
    firmware = config["fw"]
    shutil.copyfile(
        os.path.join(firmware["dir"], firmware["code"]),
        os.path.join(args.dir, firmware["code"]),
    )
    shutil.copyfile(
        os.path.join(firmware["dir"], firmware["vars"]),
        os.path.join(args.dir, firmware["vars"]),
    )

    # Create disk image
    create_disk_image(args)


def build_and_inject_stubble(args, config):
    """Build lace-stubble and inject it into the VM disk image"""

    # Build lace-stubble
    stubble_target = f"{config['arch']}-unknown-uefi"
    subprocess.run(
        [
            "cargo",
            "build",
            "-p",
            "lace-stubble",
            "--no-default-features",
            "--features",
            "efi",
            "--target",
            stubble_target,
        ],
        check=True,
    )

    # Open disk image for read/write
    disk_image_path = os.path.join(args.dir, "disk.img")
    gfs = guestfs.GuestFS(python_return_dict=True)
    if DEBUG:
        gfs.set_trace(1)
    gfs.add_drive_opts(disk_image_path, format="raw", readonly=0)
    gfs.launch()

    # Find and mount OS root
    roots = gfs.inspect_os()
    if not roots:
        raise RuntimeError("No operating systems found in the Ubuntu image")
    mps = gfs.inspect_get_mountpoints(roots[0])
    for mount_point, device in sorted(mps.items(), key=lambda k: len(k[0])):
        gfs.mount(device, mount_point)

    # Download kernel and initrd
    boot_listing = gfs.ls("/boot/")
    kernel_name = max(filter(lambda n: n.startswith("vmlinuz-"), boot_listing))
    initrd_name = max(filter(lambda n: n.startswith("initrd.img-"), boot_listing))
    gfs.download(f"/boot/{kernel_name}", os.path.join(args.dir, "vmlinuz"))
    gfs.download(f"/boot/{initrd_name}", os.path.join(args.dir, "initrd.img"))

    # On arm64 strip existing stubble layer from kernel
    dtbauto_files = []
    if config["arch"] == "aarch64":
        pe = pefile.PE(os.path.join(args.dir, "vmlinuz"))
        dtbauto_idx = 0
        for section in pe.sections:
            if section.Name.rstrip(b"\x00") == b".linux":
                with open(
                    os.path.join(args.dir, "vmlinuz-really"), "wb"
                ) as vmlinuz_really:
                    vmlinuz_really.write(section.get_data())
            elif section.Name.rstrip(b"\x00") == b".dtbauto":
                dtbauto_path = os.path.join(args.dir, f"dtbauto-{dtbauto_idx}")
                with open(dtbauto_path, "wb") as dtbauto_file:
                    dtbauto_file.write(section.get_data())
                dtbauto_files.append(dtbauto_path)
                dtbauto_idx += 1
        shutil.move(
            os.path.join(args.dir, "vmlinuz-really"), os.path.join(args.dir, "vmlinuz")
        )

    # Create stubble EFI binary
    stubble_efi_path = os.path.join(
        "target", stubble_target, "debug", "lace-stubble.efi"
    )
    output_efi_path = os.path.join(args.dir, "stubble.efi")
    pewrap_cmd = [
        "cargo",
        "run",
        "-p",
        "pewrap",
        "--",
        "--stub",
        stubble_efi_path,
        "--output",
        output_efi_path,
        "--linux",
        os.path.join(args.dir, "vmlinuz"),
        "--initrd",
        os.path.join(args.dir, "initrd.img"),
        "--cmdline",
        f"console=ttyS0 console=tty0 root=UUID={gfs.vfs_uuid(roots[0])} rw",
        "--hwids",
        "data/hwids/json",
    ]
    # Add dtbauto files we might have extracted above
    if dtbauto_files:
        for dtbauto_file in dtbauto_files:
            pewrap_cmd.extend(["--dtbauto", dtbauto_file])
    subprocess.run(pewrap_cmd, check=True)

    # Copy EFI binary to ESP
    gfs.mkdir_p("/boot/efi/EFI/BOOT")
    gfs.upload(
        output_efi_path, f"/boot/efi/EFI/BOOT/BOOT{EFI_SUFFIXES[config['arch']]}"
    )

    # Close disk
    gfs.umount_all()
    gfs.shutdown()
    gfs.close()


def build_and_inject_speedboot(args, config):
    """Replace BOOT{EFI_SUFFIXES[config['arch']]}.efi with lace-speedboot"""

    print("Building lace-speedboot...")
    subprocess.run(
        [
            "cargo",
            "build",
            "-p",
            "lace-speedboot",
            "--no-default-features",
            "--features",
            "efi",
            "--target",
            f"{config['arch']}-unknown-uefi",
        ],
        check=True,
    )

    speedboot_path = f"target/{config['arch']}-unknown-uefi/debug/lace-speedboot.efi"
    if not os.path.exists(speedboot_path):
        raise RuntimeError(f"Build failed: {speedboot_path} not found")

    print(f"Built: {speedboot_path}")

    print("Injecting lace-speedboot into disk image...")
    disk_image_path = os.path.join(args.dir, "disk.img")
    gfs = guestfs.GuestFS(python_return_dict=True)
    gfs.add_drive_opts(disk_image_path, format="raw", readonly=0)
    gfs.launch()

    # Find and mount partitions
    roots = gfs.inspect_os()
    if not roots:
        raise RuntimeError("No OS found in disk")

    mps = gfs.inspect_get_mountpoints(roots[0])
    for mount_point, device in sorted(mps.items(), key=lambda k: len(k[0])):
        gfs.mount(device, mount_point)

    # Copy EFI binary to ESP
    gfs.mkdir_p("/boot/efi/EFI/BOOT")
    gfs.upload(speedboot_path, f"/boot/efi/EFI/BOOT/BOOT{EFI_SUFFIXES[config['arch']]}")

    # List grub configs for debugging
    print("\nGRUB configs found:")
    for grub_cfg in ["/boot/grub/grub.cfg", "/boot/efi/EFI/ubuntu/grub.cfg"]:
        if gfs.exists(grub_cfg):
            print(f"  {grub_cfg}")

    gfs.umount_all()
    gfs.shutdown()
    gfs.close()

    print("Injection complete")


def build_and_inject_virt(args, config, package):
    """Build and inject a virt platform payload into a CBFS flash image"""

    if config["arch"] != "x86_64":
        raise ValueError(f"{package} only supports x86_64 on virt")

    print(f"Building {package} for virt platform...")

    # 1. Build bootblock
    subprocess.run(
        [
            "cargo", "build", "--release",
            "--manifest-path", "lace-platform/src/virt/bootblock/Cargo.toml",
            "--target", "lace-platform/src/virt/bootblock/i686-bootblock.json",
            "-Z", "build-std=core,alloc",
            "-Z", "build-std-features=compiler-builtins-mem",
            "-Z", "json-target-spec",
        ],
        check=True,
    )

    # 2. Convert bootblock ELF to flat binary
    subprocess.run(
        [
            "llvm-objcopy", "-O", "binary",
            "target/i686-bootblock/release/virt-bootblock",
            os.path.join(args.dir, "bootblock.bin"),
        ],
        check=True,
    )

    # 3. Build firmware
    subprocess.run(
        [
            "cargo", "build", "-p", package,
            "--no-default-features", "--features", "virt",
            "--target", "lace-platform/src/virt/x86_64-virt.json",
            "-Z", "json-target-spec",
            "-Z", "build-std=core,alloc",
            "-Z", "build-std-features=compiler-builtins-mem",
        ],
        check=True,
    )

    firmware_elf = f"target/x86_64-virt/debug/{package}"
    if not os.path.exists(firmware_elf):
        raise RuntimeError(f"Build failed: {firmware_elf} not found")

    # 4. Create CBFS flash image
    flash_image = os.path.join(args.dir, "flash.bin")
    subprocess.run(
        [
            "cargo", "run", "-p", "flashedit", "--",
            "create",
            "-o", flash_image,
            "-s", "16M",
            "-b", os.path.join(args.dir, "bootblock.bin"),
            "-f", f"fallback/payload={firmware_elf}",
        ],
        check=True,
    )

    print(f"Flash image: {flash_image}")
    print("Injection complete")


def build_and_inject_bios(args, config, package):
    """Build and inject a BIOS payload (lace-speedboot)"""

    if config["arch"] != "x86_64":
        raise ValueError(f"{package} only supports x86_64 on BIOS")

    print(f"Building {package} for BIOS...")
    # 1. Build BIOS stages
    subprocess.run(["make", "-C", "lace-platform/src/bios/boot/"], check=True)

    # 2. Build Core
    cargo_cmd = [
        "cargo",
        "build",
        "-p",
        package,
        "--no-default-features",
        "--features",
        "bios",
        "--target",
        "lace-platform/src/bios/x86_64-bios.json",
        "-Z", "json-target-spec",
        "-Z",
        "build-std=core,alloc,compiler_builtins",
        "-Z",
        "build-std-features=compiler-builtins-mem",
        "--release",
    ]

    subprocess.run(cargo_cmd, check=True)

    stage1_bin = "lace-platform/src/bios/boot/stage1.bin"
    stage2_bin = "lace-platform/src/bios/boot/stage2.bin"
    core_bin = f"target/x86_64-bios/release/{package}"
    disk_img = os.path.join(args.dir, "disk.img")

    if not os.path.exists(disk_img):
        raise RuntimeError(f"{disk_img} not found")

    try:
        with open(disk_img, "r+b") as disk_f:
            # 3. Write Stage 1 to LBA 0
            print(f"Writing {stage1_bin} to {disk_img} (LBA 0)")
            try:
                with open(stage1_bin, "rb") as f:
                    stage1_data = f.read()

                if len(stage1_data) > 440:
                    raise RuntimeError(
                        f"Stage 1 binary size ({len(stage1_data)} bytes) exceeds 440 bytes."
                    )

                disk_f.seek(0)
                disk_f.write(stage1_data)
            except FileNotFoundError:
                raise RuntimeError(f"Could not read {stage1_bin}")

            # 4. Find BIOS Boot Partition using GPT class
            print("Parsing GPT...")
            try:
                gpt = GPT(disk_f)
                gpt.read()
            except Exception as e:
                raise RuntimeError(f"Error parsing GPT: {e}")

            partition = gpt.find_partition_by_type(BIOS_BOOT_PARTITION_TYPE_GUID)

            if partition is None:
                raise RuntimeError(
                    f"BIOS boot partition (GUID {BIOS_BOOT_PARTITION_TYPE_GUID}) not found."
                )

            target_offset = partition.first_lba * 512
            print(f"Found BIOS boot partition: {partition}")
            print(f"Target offset: {target_offset}")

            # 5. Write Stage 2 + Core to BIOS Boot Partition
            print(
                f"Writing {stage2_bin} + {core_bin} to {disk_img} (Offset {target_offset})"
            )
            try:
                with open(core_bin, "rb") as f:
                    core_data = f.read()

                core_size = len(core_data)
                print(f"Core size: {core_size} bytes")

                with open(stage2_bin, "rb") as f:
                    stage2_data = bytearray(f.read())

                # Patch core size at offset 8 (Little Endian 32-bit integer)
                if len(stage2_data) >= 12:
                    struct.pack_into("<I", stage2_data, 8, core_size)
                    print(f"Patched Stage 2 with Core size: {core_size}")
                else:
                    print("Warning: Stage 2 binary too small to patch size.")

                # Pad Stage 2 to 2KB (4 sectors)
                if len(stage2_data) > 2048:
                    raise RuntimeError(
                        f"Stage 2 binary size ({len(stage2_data)} bytes) exceeds 2KB."
                    )

                stage2_padded = stage2_data + b"\x00" * (2048 - len(stage2_data))

                combined_data = stage2_padded + core_data
                combined_size = len(combined_data)
                partition_size = (partition.last_lba - partition.first_lba + 1) * 512

                if combined_size > partition_size:
                    raise RuntimeError(
                        f"Combined binary size ({combined_size} bytes) exceeds partition size ({partition_size} bytes)."
                    )

                disk_f.seek(target_offset)
                disk_f.write(combined_data)
            except FileNotFoundError:
                raise RuntimeError("Could not read binaries")
    except IOError as e:
        raise RuntimeError(f"Error opening {disk_img}: {e}")

    print("Injection complete")


def do_start(args):
    """Handler for the 'start' command"""

    # Load VM configuration
    with open(
        os.path.join(args.dir, "config.json"), "r", encoding="utf-8"
    ) as config_file:
        config = json.load(config_file)

    # Build and inject lace-stubble
    match args.app:
        case "stubble":
            build_and_inject_stubble(args, config)
        case "speedboot":
            build_and_inject_speedboot(args, config)
        case "speedboot-bios":
            build_and_inject_bios(args, config, package="lace-speedboot")
        case "speedboot-virt":
            build_and_inject_virt(args, config, package="lace-speedboot")
        case _:
            raise ValueError(f"Unknown app: {args.app}")

    # Start swtpm if requested
    swtpm_proc = None
    tpm_sock = None
    if config.get("tpm"):
        if not shutil.which("swtpm"):
            raise RuntimeError("swtpm not found")

        tpm_dir = os.path.join(args.dir, "tpm")
        os.makedirs(tpm_dir, exist_ok=True)
        tpm_sock = os.path.join(tpm_dir, "swtpm-sock")

        # Clean up stale socket
        if os.path.exists(tpm_sock):
            os.remove(tpm_sock)

        swtpm_cmd = [
            "swtpm",
            "socket",
            "--tpmstate",
            f"dir={tpm_dir}",
            "--ctrl",
            f"type=unixio,path={tpm_sock}",
            "--tpm2",
            "--log",
            "level=0",
        ]
        print(f"Starting swtpm...")
        swtpm_proc = subprocess.Popen(swtpm_cmd)

        # Wait for socket
        retries = 50
        while not os.path.exists(tpm_sock) and retries > 0:
            time.sleep(0.1)
            retries -= 1

        if not os.path.exists(tpm_sock):
            if swtpm_proc.poll() is not None:
                raise RuntimeError("swtpm exited unexpectedly")
            raise RuntimeError("Timed out waiting for swtpm socket")

    try:
        # Check for acpi disable on ARM64
        acpi_flag = ""
        if config["arch"] == "aarch64" and config.get("acpi") == "off":
            acpi_flag = ",acpi=off"

        # Construct QEMU command
        qemu_cmd = [
            "qemu-system-" + config["arch"],
            "-nographic",
            "-machine",
            f"{config['machine']},accel={best_vm_accel(config['arch'])}{acpi_flag}",
            "-cpu",
            config["cpu"]["model"],
            "-smp",
            f"cores={config['cpu']['cores']}",
            "-m",
            config["ram"],
        ]

        if args.gdb:
            qemu_cmd.extend(["-s", "-S"])
            # Force TCG when debugging since KVM does not support single-step
            # or proper breakpoints in real mode.
            qemu_cmd[qemu_cmd.index("-machine") + 1] = qemu_cmd[
                qemu_cmd.index("-machine") + 1
            ].replace(f"accel={best_vm_accel(config['arch'])}", "accel=tcg")
            print("QEMU gdb stub enabled on tcp::1234 (TCG mode)")

        if args.app in ["speedboot-virt"]:
            # Virt platform: CBFS flash as BIOS, modern virtio-blk
            qemu_cmd.extend(
                [
                    "-bios",
                    os.path.join(args.dir, "flash.bin"),
                    "-blockdev",
                    f"driver={config['disk']['format']},node-name=disk,file.driver=file,file.filename="
                    + os.path.join(args.dir, config["disk"]["file"]),
                    "-device",
                    "virtio-blk-pci,drive=disk,disable-legacy=on",
                ]
            )
        elif args.app in ["speedboot-bios"]:
            # Legacy BIOS boot
            qemu_cmd.extend(
                [
                    "-drive",
                    f"if=virtio,file={os.path.join(args.dir, config['disk']['file'])},format={config['disk']['format']}",
                ]
            )
        else:
            # UEFI boot
            qemu_cmd.extend(
                [
                    "-drive",
                    "if=pflash,unit=0,format=raw,readonly=on,file="
                    + os.path.join(args.dir, config["fw"]["code"]),
                    "-drive",
                    "if=pflash,unit=1,format=raw,file="
                    + os.path.join(args.dir, config["fw"]["vars"]),
                    "-drive",
                    f"if=none,id=disk,format={config['disk']['format']},file="
                    + os.path.join(args.dir, config["disk"]["file"]),
                    "-device",
                    "virtio-blk-pci,drive=disk,bootindex=1",
                ]
            )

        # Add TPM if requested
        if config.get("tpm"):
            tpm_dev = "tpm-tis-device" if config["arch"] == "aarch64" else "tpm-tis"
            qemu_cmd.extend(
                [
                    "-chardev",
                    f"socket,id=chrtpm,path={tpm_sock}",
                    "-tpmdev",
                    "emulator,id=tpm0,chardev=chrtpm",
                    "-device",
                    f"{tpm_dev},tpmdev=tpm0",
                ]
            )

        # Add SMBIOS table to QEMU command
        if "smbios" in config:
            qemu_cmd.extend(
                ["-smbios", "file=" + os.path.join(args.dir, config["smbios"])]
            )

        # Install EDID file using fakeedid.efi in a vvfat drive
        if "edid" in config:
            # Build fakeedid.efi
            subprocess.run(
                [
                    "cargo",
                    "build",
                    "-p",
                    "fakeedid",
                    "--target",
                    f"{config['arch']}-unknown-uefi",
                ],
                check=True,
            )
            # Create EDID drive
            edid_drive = os.path.join(args.dir, "edid_drive")
            os.makedirs(os.path.join(edid_drive, "EFI", "BOOT"), exist_ok=True)
            shutil.copyfile(
                os.path.join(
                    "target",
                    f"{config['arch']}-unknown-uefi",
                    "debug",
                    "fakeedid.efi",
                ),
                os.path.join(
                    edid_drive, "EFI", "BOOT", f"BOOT{EFI_SUFFIXES[config['arch']]}"
                ),
            )
            shutil.copyfile(
                os.path.join(args.dir, config["edid"]),
                os.path.join(edid_drive, "edid.bin"),
            )
            qemu_cmd.extend(
                [
                    "-drive",
                    f"file=fat:rw:{edid_drive},format=raw,if=none,id=edid_drive",
                    "-device",
                    "virtio-blk-pci,drive=edid_drive,bootindex=0",
                ]
            )

        # Add cloud-init seed on first boot
        if not os.path.exists(os.path.join(args.dir, "cloud-init-seed.img")):
            cloud_init_iso_path = os.path.join(args.dir, "cloud-init-seed.img")
            with open(
                os.path.join(args.dir, "user-data"), "w", encoding="utf-8"
            ) as file:
                file.write(CI_USER_DATA)
            subprocess.run(
                [
                    "cloud-localds",
                    cloud_init_iso_path,
                    os.path.join(args.dir, "user-data"),
                ],
                check=True,
            )
            qemu_cmd.extend(
                ["-drive", f"file={cloud_init_iso_path},format=raw,if=virtio"]
            )

        # Start the VM
        subprocess.run(qemu_cmd, check=True)

    finally:
        if swtpm_proc:
            print("Terminating swtpm...")
            swtpm_proc.terminate()
            swtpm_proc.wait()

def main():
    """Main function to parse arguments and execute commands"""

    parser = argparse.ArgumentParser(description="Lace VM management script")
    parser.add_argument(
        "--dir", type=str, default=default_vm_dir(), help="Directory for VM files"
    )

    cmds = parser.add_subparsers(dest="command", required=True)

    init_cmd = cmds.add_parser("init", help="Initialize the VM")
    init_cmd.add_argument(
        "--arch", type=str, default=platform.machine(), help="Target architecture"
    )
    init_cmd.add_argument("--cores", type=int, default=1, help="Number of CPU cores")
    init_cmd.add_argument("--ram", type=str, default="1G", help="Amount of RAM")
    init_cmd.add_argument(
        "--disk-size", type=parse_disk_size, default="4G", help="Disk size for the VM"
    )
    init_cmd.add_argument(
        "--root-fs-type",
        type=str,
        default="ext4",
        help="Filesystem type for root partition",
    )

    start_cmd = cmds.add_parser("start", help="Start the VM")
    start_cmd.add_argument(
        "--app", type=str, default="stubble", help="App to run (stubble, speedboot, speedboot-bios, speedboot-virt)"
    )
    start_cmd.add_argument(
        "--gdb", action="store_true", help="Enable QEMU gdb stub on tcp::1234"
    )

    args = parser.parse_args()

    match args.command:
        case "init":
            do_init(args)
        case "start":
            do_start(args)
        case _:
            raise RuntimeError("executed unreachable code")


if __name__ == "__main__":
    main()

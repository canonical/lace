#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only

"""Lace test VM management script"""

import argparse
import copy
import json
import os
import platform
import shutil
import subprocess
import guestfs
import requests

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
    gfs.part_add(device, "p", 2048, 2048 + (512 * MIB // SECTOR_SIZE) - 1)  # ESP
    gfs.part_set_gpt_type(device, 1, EFI_SYSTEM_PARTITION_TYPE_GUID)
    gfs.part_add(
        device, "p", 2048 + (512 * MIB // SECTOR_SIZE), -2048
    )  # Root partition

    # Format filesystems
    partitions = list(filter(lambda s: s.startswith(device), gfs.list_partitions()))
    esp_partition = partitions[0]
    root_partition = partitions[1]
    gfs.mkfs("vfat", esp_partition)
    gfs.mkfs(args.root_fs_type, root_partition)

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

    # Remove GRUB installation and EFI files
    gfs.rm_rf("/disk/boot/grub")
    gfs.rm_rf("/disk/boot/efi/EFI")

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

    # On arm64 strip existing stubble layer from kernel using objcopy
    if config["arch"] == "aarch64":
        subprocess.run(
            [
                "aarch64-linux-gnu-objcopy",
                "--dump-section",
                ".linux=" + os.path.join(args.dir, "vmlinuz-really"),
                os.path.join(args.dir, "vmlinuz"),
                os.path.join(args.dir, "unused"),
            ],
            check=True,
        )
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
        f"console=tty0 root=UUID={gfs.vfs_uuid(roots[0])} rw",
        "--hwids",
        "data/hwids/json",
    ]
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


def do_start(args):
    """Handler for the 'start' command"""

    # Load VM configuration
    with open(
        os.path.join(args.dir, "config.json"), "r", encoding="utf-8"
    ) as config_file:
        config = json.load(config_file)

    # Build and inject lace-stubble
    build_and_inject_stubble(args, config)

    # Check for acpi disable on ARM64
    acpi_flag = ""
    if config["arch"] == "aarch64" and config.get("acpi") == "off":
        acpi_flag = ",acpi=off"

    # Construct QEMU command
    qemu_cmd = [
        "qemu-system-" + config["arch"],
        "-machine",
        f"{config['machine']},accel={best_vm_accel(config['arch'])}{acpi_flag}",
        "-cpu",
        config["cpu"]["model"],
        "-smp",
        f"cores={config['cpu']['cores']}",
        "-m",
        config["ram"],
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

    # Add SMBIOS table to QEMU command
    if "smbios" in config:
        qemu_cmd.extend(["-smbios", "file=" + os.path.join(args.dir, config["smbios"])])

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
            os.path.join(args.dir, config["edid"]), os.path.join(edid_drive, "edid.bin")
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
        with open(os.path.join(args.dir, "user-data"), "w", encoding="utf-8") as file:
            file.write(CI_USER_DATA)
        subprocess.run(
            [
                "cloud-localds",
                cloud_init_iso_path,
                os.path.join(args.dir, "user-data"),
            ],
            check=True,
        )
        qemu_cmd.extend(["-drive", f"file={cloud_init_iso_path},format=raw,if=virtio"])

    # Start the VM
    subprocess.run(qemu_cmd, check=True)


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

    cmds.add_parser("start", help="Start the VM")

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

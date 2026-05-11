#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
# Copyright (C) 2026, Canonical Ltd.

"""Build Lace EFI binaries for supported architectures."""

import argparse
import os
import subprocess


RISCV_TARGET = "riscv64imac-unknown-none-elf"
RISCV_BUILD_STD = "build-std=core,alloc,compiler_builtins"
RISCV_BUILD_STD_FEATURES = "build-std-features=mem"


def parse_args():
    """Parse command line arguments."""
    parser = argparse.ArgumentParser(description="Build EFI binary for Lace package")
    parser.add_argument(
        "--package", required=True, choices=["lace-stubble", "lace-speedboot"]
    )
    parser.add_argument(
        "--arch", required=True, choices=["x86_64", "aarch64", "riscv64"]
    )
    parser.add_argument(
        "--profile", default="debug", choices=["debug", "release"], help="Build profile"
    )
    return parser.parse_args()


def efi_target(arch):
    """Return the build target triple for an architecture."""
    if arch == "riscv64":
        return RISCV_TARGET
    return f"{arch}-unknown-uefi"


def efi_output_path(package, arch, profile):
    """Return the expected EFI output path for a build."""
    target = efi_target(arch)
    profile_dir = "release" if profile == "release" else "debug"
    return os.path.join("target", target, profile_dir, f"{package}.efi")


def build_native_uefi(package, arch, profile):
    """Build an EFI binary through rustc's native UEFI targets."""
    cmd = [
        "cargo",
        "build",
        "-p",
        package,
        "--no-default-features",
        "--features",
        "efi",
        "--target",
        efi_target(arch),
    ]
    if profile == "release":
        cmd.append("--release")
    subprocess.run(cmd, check=True)


def build_riscv_elf(package, profile):
    """Build a RISC-V PIE ELF image suitable for PE conversion."""
    env = os.environ.copy()
    riscv_rustflags = " ".join(
        [
            "-C relocation-model=pie",
            "-C link-arg=-pie",
            "-C link-arg=--entry=efi_main",
            "-C link-arg=-z",
            "-C link-arg=common-page-size=4096",
            "-C link-arg=-z",
            "-C link-arg=max-page-size=4096",
            "-C link-arg=-z",
            "-C link-arg=noexecstack",
            "-C link-arg=-z",
            "-C link-arg=relro",
            "-C link-arg=-z",
            "-C link-arg=separate-code",
        ]
    )
    current = env.get("CARGO_TARGET_RISCV64IMAC_UNKNOWN_NONE_ELF_RUSTFLAGS", "")
    env["CARGO_TARGET_RISCV64IMAC_UNKNOWN_NONE_ELF_RUSTFLAGS"] = (
        f"{current} {riscv_rustflags}".strip()
    )
    env["RUSTC_BOOTSTRAP"] = "1"

    cmd = [
        "cargo",
        "build",
        "-Z",
        RISCV_BUILD_STD,
        "-Z",
        RISCV_BUILD_STD_FEATURES,
        "-p",
        package,
        "--no-default-features",
        "--features",
        "efi",
        "--target",
        RISCV_TARGET,
    ]
    if profile == "release":
        cmd.append("--release")
    subprocess.run(cmd, check=True, env=env)


def convert_riscv_to_efi(package, profile):
    """Convert the built RISC-V ELF image into a PE/EFI binary."""
    profile_dir = "release" if profile == "release" else "debug"
    elf_path = os.path.join("target", RISCV_TARGET, profile_dir, package)
    if not os.path.exists(elf_path):
        raise RuntimeError(f"Build failed: {elf_path} not found")

    efi_path = os.path.join("target", RISCV_TARGET, profile_dir, f"{package}.efi")
    scripts_dir = os.path.dirname(os.path.abspath(__file__))
    subprocess.run(
        [
            "python3",
            os.path.join(scripts_dir, "elf2efi.py"),
            "--version-major=6",
            "--version-minor=16",
            "--efi-major=1",
            "--efi-minor=1",
            "--subsystem=10",
            "--minimum-sections=2048",
            "--copy-sections=.sbat",
            elf_path,
            efi_path,
        ],
        check=True,
    )


def main():
    """Run the EFI build flow and print resulting output path."""
    args = parse_args()
    if args.arch == "riscv64":
        build_riscv_elf(args.package, args.profile)
        convert_riscv_to_efi(args.package, args.profile)
    else:
        build_native_uefi(args.package, args.arch, args.profile)

    output = efi_output_path(args.package, args.arch, args.profile)
    if not os.path.exists(output):
        raise RuntimeError(f"Build failed: {output} not found")
    print(output)


if __name__ == "__main__":
    main()

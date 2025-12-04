#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only

"""Cargo wrapper for CI environments, splits packages into multiple runs
to avoid feature unification."""

import argparse
import subprocess
import sys
import json


def get_workspace_members() -> set[str]:
    """Return a set of workspace member package names."""
    metadata = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        check=True,
        capture_output=True,
        text=True,
    )
    return set(map(lambda pkg: pkg["name"], json.loads(metadata.stdout)["packages"]))


def main():
    """Main function to parse arguments and run cargo commands."""

    parser = argparse.ArgumentParser(description="Cargo wrapper for CI environments")
    parser.add_argument("command", help="Cargo command to run")
    parser.add_argument(
        "--workspace", action="store_true", help="Run command for all workspace members"
    )
    parser.add_argument(
        "-p", "--package", action="append", help="Specify package to run command for"
    )
    parser.add_argument(
        "--exclude", action="append", help="Exclude package from workspace run"
    )
    args, unknown_args = parser.parse_known_args()

    package_set = set(args.package) if args.package else set()
    exclude_set = set(args.exclude) if args.exclude else set()

    if args.workspace and package_set:
        print("Error: --package cannot be used with --workspace", file=sys.stderr)
        sys.exit(1)

    if not args.workspace and exclude_set:
        print("Error: --exclude can only be used with --workspace", file=sys.stderr)
        sys.exit(1)

    if args.workspace:
        package_set = get_workspace_members().difference(exclude_set)

    if package_set:
        for package in package_set:
            result = subprocess.run(
                ["cargo", args.command, "-p", package] + unknown_args, check=False
            )
            if result.returncode != 0:
                sys.exit(result.returncode)
    else:
        result = subprocess.run(["cargo", args.command] + unknown_args, check=False)
        if result.returncode != 0:
            sys.exit(result.returncode)


if __name__ == "__main__":
    main()

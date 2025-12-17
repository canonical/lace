# Copilot Instructions for Lace

## Project Overview

**Lace** is a Rust-based firmware development project focused on building bootloaders and firmware for embedded systems. The project integrates Rust code with U-Boot's C codebase through a library interface called "ulib".

### Key Components

- **lace-boot**: Bootloader implementation supporting multiple platforms (sandbox, EFI, bare metal)
- **lace-util**: Core utility library with hardware identification (CHID), SMBIOS parsing, EDID handling, PE image parsing, and other firmware-related utilities
- **lace-platform**: Platform abstraction layer for different firmware environments
- **lace-util-derive**: Procedural macros for enum utilities
- **tools/**: Helper utilities (pewrap, collect-hwids, fakeedid)
- **ulib/**: U-Boot C codebase (external dependency - DO NOT MODIFY)

### Build System

The project uses a **Cargo workspace** for Rust code and a **custom Makefile** for lace-boot builds that integrate with U-Boot.

**Two build variants exist:**
- **Standard builds**: Link with ulib (U-Boot library) for full firmware functionality
- **Pure builds**: Standalone Rust-only binaries (controlled by `pure` feature flag)

**Supported platforms:**
- `sandbox`: Native Linux binary for testing and development
- `efi`: UEFI applications (efi-x86_app64, efi-x86_app32)
- `metal`: Bare metal ROM images (qemu-x86)

## Code Style and Conventions

### Follow STYLE.md Rigorously

The project has a detailed style guide in `STYLE.md`. Key points:

#### Comments
- Module-level `//!` doc comments explain purpose and design
- Public items require `///` doc comments
- Reference specifications when relevant (e.g., "per RFC 9562", "see SMBIOS spec §7.1")
- Aim for 80 column width
- **Don't comment obvious code** - comment the "why", not the "what"

#### Naming
- Constants: `SCREAMING_SNAKE_CASE`
- Types: `PascalCase`
- Functions/variables: `snake_case`
- Boolean functions: prefix with `is_`, `has_`, `can_`
- Use domain terminology consistently: `chid`, `smbios`, `guid`, `edid`

#### Integer Constants
- Avoid explicit type suffixes unless necessary
- Use underscores for readability: `0x1234_5678`, `1_000_000`
- Use lowercase hex (`0xabc`) for bit patterns, flags, addresses
- Use decimal for counts, sizes, human-meaningful quantities
- Prefer the `bitflags` crate for type-safe flag handling

#### Code Organization
1. Imports at top (formatted by cargo fmt)
2. Constants next
3. Type declarations
4. Functions (public before private)
5. Tests at bottom in `#[cfg(test)] mod test`
6. **Important**: Do not rearrange existing code when making changes

#### Tests
- Naming: `#[test] fn test_<function>_<scenario>()`
- Place unit tests in `#[cfg(test)] mod test` at file bottom
- Use specific assertions with context: `assert_eq!(a, b, "CHID type {} mismatch", i)`
- Test happy path, edge cases, and error conditions

### Licensing

All Lace code (excluding `ulib/`) uses dual licensing:

**Required header for all Rust source files:**
```rust
// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Name <email@canonical.com>
```

**For Python/Shell scripts:**
```python
#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
# Copyright (C) 2025, Canonical Ltd.
```

**Note**: Files in `ulib/` use GPL-2.0+ and should not be modified as they are part of the U-Boot codebase.

## Building and Testing

### Quick Build Commands

```bash
# Run all pre-commit checks (formatting, linting, compilation, and tests)
pre-commit run --all-files

# Format code only (if not using pre-commit)
./scripts/cargo_ci.py fmt --all

# Run tests only (if not using pre-commit)
./scripts/cargo_ci.py test --workspace --exclude lace-stubble --exclude fakeedid --exclude u-boot-sys

# Only run these if you modified lace-boot:
# Test all board configurations (lace-boot changes only)
./scripts/test-builds.sh

# Build lace-boot for specific board (lace-boot changes only)
make -C lace-boot config all BOARD=sandbox
make -C lace-boot config all BOARD=efi-x86_app64
```

**Note**: Pre-commit hooks automatically run cargo fmt, cargo check, cargo clippy, and cargo test, so you typically only need to run `pre-commit run --all-files` before committing.

### CI Pipeline

GitHub Actions runs four jobs:
1. **check**: `cargo check` for all workspace members
2. **fmt**: Format checking with `rustfmt`
3. **clippy**: Linting with **all warnings as errors** (`-D warnings`)
4. **test**: Unit tests

### The cargo_ci.py Wrapper

The `./scripts/cargo_ci.py` wrapper is used instead of direct `cargo` commands because:
- It splits packages into multiple runs to avoid Cargo feature unification issues
- Use `--workspace` to run on all members, `--exclude` to skip specific crates
- Always exclude `lace-boot` from most workspace commands (it has special build requirements)

## Common Issues and Workarounds

### binman Conflicts

**Issue**: If you have a 'binman' tool from another source (e.g., a git binary manager), it may conflict with U-Boot's binman tool required for qemu-x86 builds.

**Solution**: Remove conflicting binman from PATH or ensure U-Boot's binman (from `ulib/tools/binman`) takes precedence.

### Profile Warnings

You may see warnings like:
```
warning: profiles for the non root package will be ignored, specify profiles at the workspace root
```

This is **expected** and can be ignored. The `panic = "abort"` setting is defined at workspace root in `Cargo.toml`.

### Pure vs Standard Builds

- **Pure builds** are Rust-only and don't link with ulib
- **Standard builds** require ulib and link with U-Boot C code
- qemu-x86 **does not support pure builds** (needs ulib for startup code)
- Use the `pure` feature flag to control which variant is built

### No-std Environment

Most crates use `#![no_std]` as they target firmware environments:
- Use `extern crate alloc;` for heap allocations
- Import from `core::` instead of `std::`
- `lace-util` has an optional `std` feature (default enabled)

## Domain-Specific Knowledge

### Key Concepts

- **CHID (Computer Hardware ID)**: GUIDs derived from system information (SMBIOS, EDID) used for firmware updates (fwupd/LVFS) and device tree selection. Follows Microsoft's CHID specification and RFC 9562.

- **SMBIOS**: System Management BIOS tables containing system information

- **EDID**: Extended Display Identification Data for monitors/panels

- **GUID/UUID**: Globally Unique Identifiers used throughout firmware

- **DTB/FDT**: Device Tree Blob/Flattened Device Tree for hardware description

- **PE Image**: Portable Executable format (used for EFI binaries)

### Critical Files to Understand

- `lace-util/src/chid.rs`: CHID computation algorithm
- `lace-util/src/chid_mapping.rs`: Device tree compatible string mapping
- `lace-util/src/smbios.rs`: SMBIOS table parsing
- `lace-util/src/edid.rs`: EDID parsing
- `lace-boot/build.rs`: Build script handling different targets and ulib linking
- `lace-boot/Makefile`: Complex build system integrating Cargo with U-Boot builds

## Commit Message Format

Follow the conventions in `CONTRIBUTING.md`:

```
component: Short summary in imperative mood

Longer explanation of what and why (not how). Wrap at 72 columns.
Reference issues or specs as needed.
```

### Component Prefixes

- Crate-specific: `lace-util/chid`, `lace-boot`, `lace-platform/efi`
- Repo-wide: `doc`, `ci`, `build`
- Scripts: `scripts` or script name

### Rules

- Capitalize first word after component prefix
- Use present/imperative tense ("Add feature" not "Added feature")
- First line ≤50 chars ideally, 72 max
- No `Signed-off-by` (but you may GPG sign with `git commit -S`)

### Examples

- `lace-util/chid: Add support for EDID panel source`
- `lace-util/smbios: Fix parsing of type 3 tables`
- `ci: Update Rust toolchain to 1.75`

## Development Workflow

### Making Changes

1. **Understand the codebase first** - Read related code and tests
2. **Follow the style guide** - Check `STYLE.md` and existing code patterns
3. **Add/update tests** - Unit tests in `#[cfg(test)] mod test`
4. **Run pre-commit checks before committing**: `pre-commit run --all-files` (runs formatting, linting, compilation checks, and unit tests)
5. **If you modified lace-boot**: Run `./scripts/test-builds.sh` or build specific boards with `make -C lace-boot config all BOARD=<board>`
6. **Commit with proper message format**

**Note**: Pre-commit hooks are configured in `.pre-commit-config.yaml` and run automatically on `git commit` if installed. They include cargo fmt, cargo check, cargo clippy, cargo test, and file quality checks. Always run `pre-commit run --all-files` before using **report_progress** (an internal progress-reporting command used in some automated workflows) to ensure all checks pass; if you do not use that workflow, you can ignore this part of the note.

### Before Submitting PR

- [ ] Code follows STYLE.md
- [ ] All files have proper license headers
- [ ] Tests added for new functionality
- [ ] Pre-commit hooks pass (`pre-commit run --all-files`)
- [ ] Commit messages follow conventions

## Safety and Error Handling

- Document safety invariants for `unsafe` blocks
- Use `Result` and `Option` types appropriately
- Avoid panics in production code paths (remember: `panic = "abort"`)
- For `no_std` code, consider whether panics are acceptable

## Dependencies

- Minimize new dependencies
- Prefer well-maintained crates from the ecosystem
- For `no_std` compatibility, check that dependencies support it
- Use `default-features = false` where appropriate

### Common Dependencies

- `zerocopy`: Zero-copy parsing of binary structures
- `bitflags`: Type-safe bitfield handling
- `fdt`: Flattened Device Tree parsing
- `uefi`, `uefi-raw`: UEFI API bindings
- `clap`: CLI argument parsing (for tools)

## Useful References

- [Project Style Guide](STYLE.md)
- [Contributing Guidelines](CONTRIBUTING.md)
- [Canonical Rust Best Practices](https://canonical.github.io/rust-best-practices/cosmetic-discipline.html)
- Microsoft CHID Specification
- RFC 9562 (UUID/GUID specification)
- UEFI Specification
- SMBIOS Specification

## Tips for Efficient Development

1. **Use `cargo_ci.py` instead of `cargo`** directly for consistency with CI
2. **Exclude lace-boot** from most workspace commands (it has special build requirements)
3. **Read the `lace-boot/README.md`** for detailed build system documentation
4. **Check existing tests** for patterns before writing new ones
5. **Reference specifications** in comments when implementing protocols/standards
6. **Don't rearrange code** unnecessarily - preserve existing organization
7. **Think about `no_std`** - most code runs in firmware environments without a full standard library

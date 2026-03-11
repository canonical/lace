# Copilot Instructions for Lace

## Project Overview

**Lace** is a framework for writing boot applications.

The current list of applications is:

* `lace-stubble` - a "stub" EFI binary that can be specialized into a bootable image by embedding resources (kernel, initrd, etc.) into PE sections.
* `tools/pewrap` - a tool to wrap assets into a stubble image
* `tools/collect-hwids` - a helper tool to collect hwids from a running laptop

They use the following library crates:

* `lace-platform` - An abstraction of the underlying platform (such as UEFI).
* `lace-util` - Platform-independent utilities
* `lace-util-derive` - Platform-independent macros

### Build System

The project uses a **Cargo workspace**.

**Supported platforms:**
- `mock`: Native Linux binary for testing and development
- `efi`: UEFI applications

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

All Lace code uses dual licensing (update 2025 for the current year)

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

## Building and Testing

### Quick Build Commands

```bash
# Run all pre-commit checks (formatting, linting, compilation, and tests)
pre-commit run --all-files

# Format code only (if not using pre-commit)
./scripts/cargo_ci.py fmt --all

# Run tests only (if not using pre-commit)
./scripts/cargo_ci.py test --workspace --exclude lace-stubble --exclude fakeedid
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

## Common Issues and Workarounds

### No-std Environment

Most crates use `#![no_std]` as they target firmware environments:
- Use `extern crate alloc;` for heap allocations
- Import from `core::` instead of `std::`
- `lace-util` has an optional `std` feature (default enabled)

## Commit Message Format

Follow the conventions in `CONTRIBUTING.md`:

```
component: Short summary in imperative mood

Longer explanation of what and why (not how). Wrap at 72 columns.
Reference issues or specs as needed.
```

### Component Prefixes

- Crate-specific: `lace-util/chid`, `lace-platform/efi`
- Repo-wide: `doc`, `ci`, `build`
- Scripts: `scripts` or script name

### Rules

- Capitalize first word after component prefix
- Use present/imperative tense ("Add feature" not "Added feature")
- First line <=50 chars ideally, 72 max
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
5. **Commit with proper message format**

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

## Tips for Efficient Development

1. **Use `cargo_ci.py` instead of `cargo`** directly for consistency with CI
4. **Check existing tests** for patterns before writing new ones
5. **Reference specifications** in comments when implementing protocols/standards
6. **Don't rearrange code** unnecessarily - preserve existing organization
7. **Think about `no_std`** - most code runs in firmware environments without a full standard library

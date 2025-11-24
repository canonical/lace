# Style Guide

This guide covers aspects not handled by rustfmt. See also
<https://canonical.github.io/rust-best-practices/cosmetic-discipline.html>.

## Comments

### When to comment

- Module-level `//!` doc comments explaining the module's purpose and design
- Public items (`pub fn`, `pub struct`, `pub const`) get `///` doc comments
- Complex algorithms or non-obvious logic get inline `//` comments
- Constants that represent magic values or external specifications
- Safety invariants for `unsafe` blocks

### When not to comment

- Obvious code that reads clearly on its own
- Private helper functions with self-explanatory names and few arguments
- Restating what the code does rather than why

### Comment style

- Aim for 80 columns
- Use complete sentences with proper punctuation for doc comments
- Use sentence fragments for inline comments (no trailing period for
  single-line)
- Reference specifications (e.g., "per RFC 9562", "see SMBIOS spec §7.1")

### Doc comment structure for functions

```rust
/// Verify that data matches an expected CRC32 checksum.
///
/// Longer explanation if needed.
///
/// # Examples
///
/// ```
/// let data = [0x12, 0x34, 0x56, 0x78];
/// assert!(verify_crc32(&data, 0x114));
/// ```
fn verify_crc32(data: &[u8], expected: u32) -> bool {
    ...
}
```

See <https://doc.rust-lang.org/std/fs/fn.hard_link.html> for a fuller example
(see the 'Source' link at the top).

## Tests

### Naming

```rust
#[test]
fn test_<function>_<scenario>() { }
```

### Structure

- Place unit tests in a `#[cfg(test)] mod test` at the bottom of the file
- Place larger / fuzz tests in a separate tests/ directory
- Use real-world test data where possible
- Comment test cases that come from external sources or known-good values

### What to test

- Happy path with representative inputs
- Edge cases (empty input, max values, etc.)
- Error conditions (return `None`, `Err`, etc.)
- Regression tests for fixed bugs

### Assertions

- Use specific assertion macros (`assert_eq!`, `assert_ne!`)
- Include context message: `assert_eq!(a, b, "CHID type {} mismatch", i)`

## Naming Conventions

- Constants: `SCREAMING_SNAKE_CASE`
- Types: `PascalCase`
- Functions/variables: `snake_case`
- Prefix boolean functions with `is_`, `has_`, `can_`
- Use domain terminology consistently (e.g., `chid`, `smbios`, `guid`)

## Integer Constants

### Declaration

Avoid specifying an explicit type for integer constants unless needed:

```rust
const MAGIC: 0x1234_5678;
const REG_OFFSET: u16 = 0x100;
const MAX_ENTRIES: u32 = 256;
```

Use underscores to separate groups of digits for readability:

```rust
const SIZE: 1_000_000;
const FLAGS: 0b1010_0011;
const ADDR: 0xdead_beef_cafe_babe;
```

### Hex vs decimal

- Use lower-case hex (`0xabc`) for bit patterns, flags, addresses, and hardware
  constants
- Use decimal for counts, sizes, and human-meaningful quantities

### Type suffixes

Avoid type suffixes on literals when the type is clear from context:

```rust
// Preferred: type is implied by the constant
const SIZE: 4096;

// Avoid: redundant suffix
const SIZE: u32 = 4096u32;

// OK: suffix to disambiguate in expressions
let x: 1_u64 << 32;

// OK: rely on the inferencer to get the type right
let y = 1 << 32;
```

### Bitmasks and flags

Prefer the `bitflags` crate for type-safe flag handling:

```rust
use bitflags::bitflags;

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct Permissions: u32 {
        const READ = 1 << 0;
        const WRITE = 1 << 1;
        const EXEC = 1 << 2;
    }
}

const DEFAULT_PERMS: Permissions = Permissions::READ.union(Permissions::WRITE);
```

## Code Organization

Remember that these are general guidelines, not fixed requirements. In some
cases constants and types are better with the functions which use them. Use your
discretion. When reviewing code, try to accept what is there unless you have a
good reason for a change.

- Imports go at the top of the file, as formatted by cargo
- Constants come next (assuming they don't need a type declaration)
- Type declarations after that
- Functions after that
- Public items before private items (but don't move functions in existing code
  when making them public / private)
- Tests at the bottom in `mod test`
- Note: Existing code should not be rearranged when making changes

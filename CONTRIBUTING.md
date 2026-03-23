# Contributing to lace

The lace team welcomes contributions via GitHub pull requests. To get your PR
merged, you need to sign [Canonical's contributor license agreement
(CLA)][cla].

Please follow the project [style guide](STYLE.md) for code formatting, comments
and tests.

## Commit Messages

### Format

```
component: Short summary in imperative mood

Longer explanation of what and why (not how). Wrap at 72 columns.
Reference issues or specs as needed.
```

### Component prefix

The component identifies which part of the codebase is affected:

- For crate-specific modules, use `crate/module`: `lace-util/chid`,
  `lace-lib/fdt`
- For top-level crate changes, use the crate name: `lace-util`, `lace-boot`
- For repo-wide files, use a descriptive prefix: `doc`, `ci`, `build`
- For scripts, use `scripts` or the script name: `scripts/vm-run`

Examples:
- `lace-util/chid.rs` → `lace-util/chid: ...`
- `lace-util/src/smbios/mod.rs` → `lace-util/smbios: ...`
- `lace-boot/src/main.rs` → `lace-boot: ...`
- `STYLE.md`, `README.md` → `doc: ...`
- `.github/workflows/` → `ci: ...`

### Rules

- Capitalize the first word after the component prefix
- Use present/imperative tense ("Add feature" not "Added feature")
- First line ≤50 chars ideally, 72 max
- Blank line between summary and body
- Body wrapped at 72 columns
- Do not use `Signed-off-by`
- You may sign your commits (i.e. `git commit -S`) but it is not required

### Examples

- `lace-util/chid: Add support for EDID panel source`
- `lace-util/smbios: Fix parsing of type 3 tables`
- `lace-util/lib: Refactor GUID handling for clarity`

## Development Setup

This repository uses [pre-commit](https://pre-commit.com/) hooks to ensure code
quality. To set up pre-commit hooks:

```bash
pip install pre-commit
pre-commit install
```

The hooks will run automatically on `git commit` and include:
- Code formatting checks (`cargo fmt`)
- Licensing checks (`cargo deny`)
- General file checks (trailing whitespace, YAML/TOML syntax, etc.)

**Before committing**, you can run the hooks manually to catch issues early:
```bash
pre-commit run --all-files
```

If pre-commit is installed, it will automatically run on every commit and prevent
commits with failing checks.

The following checks can be quite expensive and configuration-dependent:
- Compilation checks (`cargo check`)
- Lint checks (`cargo clippy`)
- Unit tests (`cargo test` on most workspace crates)

Please run them manually as needed.

This ensures all code meets quality standards before
being pushed to the repository.

[cla]: https://ubuntu.com/legal/contributors

# xtask

Build automation tasks for the lace project.

## Usage

Run xtask tasks with:
```none
cargo run -p xtask -- <command>
```

## Available commands

### `mangen`

Generate manual pages for all tools that have CLI interfaces.

```none
cargo run -p xtask -- mangen
```

This generates man pages in the `man/` directory at the repository root:
- `man/pewrap.1` - Man page for pewrap
- `man/collect-hwids.1` - Man page for collect-hwids

By default, man pages are written to `./man`. To specify a different output directory, use:

```none
cargo run -p xtask -- mangen --output-dir /path/to/output
```

To view the generated man pages, use the `man` command:
```none
man ./man/pewrap.1
man ./man/collect-hwids.1
```

## Adding new tools

To add man page generation for a new tool:

1. Ensure the tool uses `clap` with the `derive` feature.
2. Extract the CLI argument struct to a separate `cli.rs` module.
3. Create a `lib.rs` that re-exports the cli module: `pub mod cli;`.
4. Add the tool as a dependency in `tools/xtask/Cargo.toml`.
5. Add man page generation call in `tools/xtask/src/main.rs`.

See pewrap or collect-hwids for examples.

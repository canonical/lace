# ucs2-rs

[![UCS2 crate on crates.io](https://img.shields.io/crates/v/ucs2.svg)](https://crates.io/crates/ucs2)
[![UCS2 docs on docs.rs](https://docs.rs/ucs2/badge.svg)](https://docs.rs/ucs2)
![Licensed under MPL-2.0](https://img.shields.io/github/license/GabrielMajeri/ucs2-rs.svg)

UCS-2 handling for Rust.

Note that UCS-2 is the predecessor of [UTF-16](https://en.wikipedia.org/wiki/UTF-16).
It is a **fixed-length** encoding, and it is used for things like [UEFI](http://www.uefi.org/).

## History

This crate arose out of the needs of the [`uefi-rs`](https://github.com/GabrielMajeri/uefi-rs) crate.
The code was extracted and placed here for easier maintenance and easier reuse.

Most of the initial code has been contributed by [FredrikAleksander](https://github.com/FredrikAleksander).

## License

Licensed under the Mozilla Public License 2.0. See the [LICENSE](LICENSE) file for the full text.

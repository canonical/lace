// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.

//! Implementation logic for the `lace-util-derive` procedural macros.
//!
//! This crate contains the actual derive logic for each macro, operating on
//! [`proc_macro2::TokenStream`] so that it can be unit-tested and instrumented
//! for coverage by tools such as `cargo-tarpaulin`.
//!
//! The `lace-util-derive` crate is a thin shim that converts between
//! [`proc_macro::TokenStream`] and [`proc_macro2::TokenStream`] and delegates
//! to this crate.

pub mod entry;
pub mod named_enum;
pub mod num_enum;

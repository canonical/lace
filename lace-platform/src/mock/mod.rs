// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! Sandbox platform abstractions.

pub mod console;
pub mod fs;
pub mod hwid;
pub mod linux;
pub mod mem;
pub mod tpm2;

use lace_util::Display;

#[derive(Debug, Display)]
#[display("mock platform error")]
pub struct Error;

impl std::error::Error for Error {}

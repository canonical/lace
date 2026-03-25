// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

//! Mock filesystem implementation.

use crate::fs::probe::DiscoveredStorage;

/// Discover all storage (mock: empty).
pub fn discover_storage() -> DiscoveredStorage {
    DiscoveredStorage::new()
}

/// Discover boot storage (mock: empty).
pub fn discover_boot_storage() -> DiscoveredStorage {
    DiscoveredStorage::new()
}

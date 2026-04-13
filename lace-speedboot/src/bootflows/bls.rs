// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Julian Andres Klode <julian.klode@canonical.com>
//! Boot Loader Specification (BLS) Type 1 boot flow implementation.

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::cell::RefCell;
use lace_platform::fs::Filesystem;
use lace_util::bls::parse_bls_entry;

use super::{BootConfiguration, BootFlow, SimpleBootConfiguration};
use crate::SpeedbootError;

/// Common locations for BLS Type 1 configuration files.
///
/// BLS files are typically stored in `/boot/loader/entries/` or
/// `/loader/entries/` with a `.conf` extension.
const BLS_DIRECTORIES: &[&str] = &["loader/entries", "boot/loader/entries"];

/// Ensure a path starts with a leading slash.
fn ensure_leading_slash(path: &str) -> alloc::string::String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    }
}

/// BLS Type 1 boot flow implementation.
pub struct BlsBootFlow;

impl BlsBootFlow {
    /// Create a new BLS boot flow.
    pub fn new() -> Self {
        Self
    }
}

impl BootFlow for BlsBootFlow {
    fn discover(
        &self,
        filesystem: Rc<RefCell<Box<dyn Filesystem>>>,
    ) -> Result<Vec<Box<dyn BootConfiguration>>, SpeedbootError> {
        let mut all_configs = Vec::new();

        // Try each known BLS directory
        for bls_dir in BLS_DIRECTORIES {
            let dir_path = ensure_leading_slash(bls_dir);

            // Try to read directory contents
            let entries = match filesystem.borrow_mut().read_dir(&dir_path) {
                Ok(entries) => entries,
                Err(_) => continue, // Directory doesn't exist, try next
            };

            log::debug!("Found BLS directory: {}", bls_dir);

            // Process each .conf file in the directory
            for entry in entries {
                if !entry.name.ends_with(".conf") {
                    continue;
                }

                let file_path = format!("{}/{}", dir_path, entry.name);

                // Read and parse the BLS file
                match filesystem.borrow_mut().open_file(&file_path) {
                    Ok(mut file) => {
                        if let Ok(content_bytes) = file.read_to_end()
                            && let Ok(content) = core::str::from_utf8(&content_bytes)
                        {
                            let bls_entry = parse_bls_entry(content);

                            log::debug!("Found BLS entry: {} ({})", bls_entry.title(), entry.name);

                            // Create boot configuration
                            all_configs.push(Box::new(SimpleBootConfiguration::new(
                                bls_entry.title().to_string(),
                                bls_entry.linux,
                                bls_entry.initrd,
                                bls_entry.options,
                                Rc::clone(&filesystem),
                            ))
                                as Box<dyn BootConfiguration>);
                        }
                    }
                    Err(_) => continue, // File read error, skip
                }
            }
        }

        if all_configs.is_empty() {
            return Err(SpeedbootError::NoBootEntriesFound);
        }

        Ok(all_configs)
    }
}

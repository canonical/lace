// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Julian Andres Klode <julian.klode@canonical.com>
//! Boot Loader Specification (Type 1) file parser.
//!
//! This module provides a parser for Boot Loader Specification (BLS) Type 1
//! configuration files. These are simple key-value files used by systemd-boot,
//! GRUB2 with BLS support, and other boot loaders.
//!
//! # Format
//!
//! BLS Type 1 files use a simple line-based format:
//! - Each line contains a key-value pair: `key value`
//! - Keys and values are separated by whitespace
//! - Lines starting with `#` are comments
//! - Empty lines are ignored
//! - Values can span to the end of the line
//!
//! # Supported Keys
//!
//! - `title`: Human-readable title for the boot entry
//! - `version`: Version string (typically kernel version)
//! - `options`: Kernel command line arguments
//! - `linux`: Path to the Linux kernel image
//! - `initrd`: Path to the initrd/initramfs image
//!
//! # References
//!
//! See [Boot Loader Specification](https://uapi-group.org/specifications/specs/boot_loader_specification/)

#[cfg(feature = "std")]
use std::string::{String, ToString};

#[cfg(not(feature = "std"))]
use alloc::string::{String, ToString};

/// Represents a BLS Type 1 boot entry.
///
/// A boot entry contains the information needed to boot a Linux system:
/// title, kernel path, initrd path, and kernel command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlsEntry {
    /// Title of the boot entry
    pub title: Option<String>,
    /// Version string (typically kernel version)
    pub version: Option<String>,
    /// Path to the Linux kernel image
    pub linux: Option<String>,
    /// Path to the initrd image
    pub initrd: Option<String>,
    /// Kernel command line arguments
    pub options: Option<String>,
}

impl BlsEntry {
    /// Create a new empty BLS entry.
    pub fn new() -> Self {
        Self {
            title: None,
            version: None,
            linux: None,
            initrd: None,
            options: None,
        }
    }

    /// Get the title of this entry, or a default if not set.
    pub fn title(&self) -> &str {
        self.title.as_deref().unwrap_or("(no title)")
    }
}

impl Default for BlsEntry {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a BLS Type 1 configuration file.
///
/// Returns a `BlsEntry` containing the parsed fields. Lines with unrecognized
/// keys are silently ignored, allowing for forward compatibility with future
/// BLS extensions.
///
/// # Example
///
/// ```
/// use lace_util::bls::parse_bls_entry;
///
/// let config = r#"
/// title Fedora CoreOS 43.20260105.3.0
/// version 1
/// options root=/dev/sda1 ro quiet
/// linux /boot/vmlinuz-6.17.12-300.fc43.x86_64
/// initrd /boot/initramfs-6.17.12-300.fc43.x86_64.img
/// "#;
///
/// let entry = parse_bls_entry(config);
/// assert_eq!(entry.title(), "Fedora CoreOS 43.20260105.3.0");
/// assert_eq!(entry.linux.as_deref(), Some("/boot/vmlinuz-6.17.12-300.fc43.x86_64"));
/// ```
pub fn parse_bls_entry(content: &str) -> BlsEntry {
    let mut entry = BlsEntry::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Split on first whitespace
        let (key, value) = match line.split_once(|c: char| c.is_whitespace()) {
            Some((k, v)) => (k, v.trim()),
            None => continue, // Line with no value
        };

        match key {
            "title" => entry.title = Some(value.to_string()),
            "version" => entry.version = Some(value.to_string()),
            "options" => entry.options = Some(value.to_string()),
            "linux" => entry.linux = Some(value.to_string()),
            "initrd" => entry.initrd = Some(value.to_string()),
            _ => {} // Ignore unknown keys for forward compatibility
        }
    }

    entry
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_empty() {
        let config = "";
        let entry = parse_bls_entry(config);
        assert_eq!(entry.title(), "(no title)");
        assert_eq!(entry.linux, None);
        assert_eq!(entry.initrd, None);
        assert_eq!(entry.options, None);
    }

    #[test]
    fn test_parse_simple() {
        let config = r#"
title Test Entry
linux /boot/vmlinuz
initrd /boot/initrd.img
options root=/dev/sda1
"#;
        let entry = parse_bls_entry(config);
        assert_eq!(entry.title(), "Test Entry");
        assert_eq!(entry.linux, Some("/boot/vmlinuz".to_string()));
        assert_eq!(entry.initrd, Some("/boot/initrd.img".to_string()));
        assert_eq!(entry.options, Some("root=/dev/sda1".to_string()));
    }

    #[test]
    fn test_parse_with_comments() {
        let config = r#"
# This is a comment
title Test Entry
# Another comment
linux /boot/vmlinuz
options root=/dev/sda1
"#;
        let entry = parse_bls_entry(config);
        assert_eq!(entry.title(), "Test Entry");
        assert_eq!(entry.linux, Some("/boot/vmlinuz".to_string()));
    }

    #[test]
    fn test_parse_fedora_coreos() {
        let config = r#"
title Fedora CoreOS 43.20260105.3.0 (ostree:0)
version 1
options rw $ignition_firstboot mitigations=auto,nosmt ostree=/ostree/boot.1/fedora-coreos/190375fcbb517f7a97c80256eb5ca74122718150b97c5482e15acba1abd77ab2/0 ignition.platform.id=qemu console=tty0 console=ttyS0,115200n8
linux /boot/ostree/fedora-coreos-190375fcbb517f7a97c80256eb5ca74122718150b97c5482e15acba1abd77ab2/vmlinuz-6.17.12-300.fc43.x86_64
initrd /boot/ostree/fedora-coreos-190375fcbb517f7a97c80256eb5ca74122718150b97c5482e15acba1abd77ab2/initramfs-6.17.12-300.fc43.x86_64.img
"#;
        let entry = parse_bls_entry(config);
        assert_eq!(entry.title(), "Fedora CoreOS 43.20260105.3.0 (ostree:0)");
        assert_eq!(entry.version, Some("1".to_string()));
        assert_eq!(
            entry.linux,
            Some("/boot/ostree/fedora-coreos-190375fcbb517f7a97c80256eb5ca74122718150b97c5482e15acba1abd77ab2/vmlinuz-6.17.12-300.fc43.x86_64".to_string())
        );
        assert_eq!(
            entry.initrd,
            Some("/boot/ostree/fedora-coreos-190375fcbb517f7a97c80256eb5ca74122718150b97c5482e15acba1abd77ab2/initramfs-6.17.12-300.fc43.x86_64.img".to_string())
        );
        assert!(
            entry
                .options
                .as_ref()
                .unwrap()
                .contains("ostree=/ostree/boot.1")
        );
        assert!(
            entry
                .options
                .as_ref()
                .unwrap()
                .contains("console=ttyS0,115200n8")
        );
    }

    #[test]
    fn test_parse_multiline_options() {
        let config = r#"
title Test
options root=/dev/sda1 ro quiet splash
linux /vmlinuz
"#;
        let entry = parse_bls_entry(config);
        assert_eq!(
            entry.options,
            Some("root=/dev/sda1 ro quiet splash".to_string())
        );
    }

    #[test]
    fn test_parse_no_initrd() {
        let config = r#"
title Test Entry
linux /boot/vmlinuz
options root=/dev/sda1
"#;
        let entry = parse_bls_entry(config);
        assert_eq!(entry.title(), "Test Entry");
        assert_eq!(entry.linux, Some("/boot/vmlinuz".to_string()));
        assert_eq!(entry.initrd, None);
    }

    #[test]
    fn test_parse_unknown_keys() {
        let config = r#"
title Test Entry
linux /boot/vmlinuz
unknown_key some value
future_key another value
"#;
        let entry = parse_bls_entry(config);
        assert_eq!(entry.title(), "Test Entry");
        assert_eq!(entry.linux, Some("/boot/vmlinuz".to_string()));
        // Unknown keys should be silently ignored
    }

    #[test]
    fn test_parse_with_version() {
        let config = r#"
title Fedora 38
version 6.2.9-300.fc38.x86_64
linux /vmlinuz-6.2.9-300.fc38.x86_64
initrd /initramfs-6.2.9-300.fc38.x86_64.img
options root=UUID=1234-5678 ro
"#;
        let entry = parse_bls_entry(config);
        assert_eq!(entry.title(), "Fedora 38");
        assert_eq!(entry.version, Some("6.2.9-300.fc38.x86_64".to_string()));
    }

    #[test]
    fn test_parse_real_fedora_coreos() {
        // Load the real-world BLS entry from testdata
        let config = include_str!("../testdata/bls-entry.conf");

        let entry = parse_bls_entry(config);
        assert_eq!(entry.title(), "Fedora CoreOS 43.20260105.3.0 (ostree:0)");
        assert_eq!(entry.version, Some("1".to_string()));
        assert_eq!(
            entry.linux,
            Some("/boot/ostree/fedora-coreos-190375fcbb517f7a97c80256eb5ca74122718150b97c5482e15acba1abd77ab2/vmlinuz-6.17.12-300.fc43.x86_64".to_string())
        );
        assert_eq!(
            entry.initrd,
            Some("/boot/ostree/fedora-coreos-190375fcbb517f7a97c80256eb5ca74122718150b97c5482e15acba1abd77ab2/initramfs-6.17.12-300.fc43.x86_64.img".to_string())
        );
        assert!(
            entry
                .options
                .as_ref()
                .unwrap()
                .contains("ostree=/ostree/boot.1")
        );
        assert!(
            entry
                .options
                .as_ref()
                .unwrap()
                .contains("console=ttyS0,115200n8")
        );
    }

    #[test]
    fn test_empty_entry() {
        let config = "";
        let entry = parse_bls_entry(config);
        assert_eq!(entry.title(), "(no title)");
        assert_eq!(entry.version, None);
        assert_eq!(entry.linux, None);
        assert_eq!(entry.initrd, None);
        assert_eq!(entry.options, None);
    }

    #[test]
    fn test_default_title() {
        let entry = BlsEntry::new();
        assert_eq!(entry.title(), "(no title)");
    }

    #[test]
    fn test_unknown_keys_ignored() {
        let config = r#"
title Test Entry
unknown_key some value
another_unknown value
linux /vmlinuz
"#;
        let entry = parse_bls_entry(config);
        assert_eq!(entry.title(), "Test Entry");
        assert_eq!(entry.linux.as_deref(), Some("/vmlinuz"));
    }

    #[test]
    fn test_whitespace_handling() {
        let config = r#"
   title   Test with Spaces
linux	/vmlinuz
options  root=/dev/sda1  ro
"#;
        let entry = parse_bls_entry(config);
        assert_eq!(entry.title(), "Test with Spaces");
        assert_eq!(entry.linux.as_deref(), Some("/vmlinuz"));
        assert_eq!(entry.options.as_deref(), Some("root=/dev/sda1  ro"));
    }
}

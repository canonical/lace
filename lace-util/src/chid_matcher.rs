// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>
//! CHID to mapping matcher

use crate::Guid;
use crate::chid::{CHID_TYPES, ChidSources, compute_chid};
use crate::chid_mapping::ChidMapping;

/// CHID types used for matching mapping (most specific to least specific)
const CHID_TYPE_MATCHING_PRIORITY: [usize; 11] = [17, 16, 15, 3, 6, 8, 10, 4, 5, 7, 9];

/// Iterator that matches CHID mappings against available CHID sources according to priority
pub struct ChidMatcher<'mappings, 'mapping_str, 'srcs> {
    mappings: &'mappings [ChidMapping<'mapping_str>],
    srcs: &'srcs ChidSources,
    mappings_idx: usize,
    type_priority_idx: usize,
    current_chid: Option<Guid>,
}

impl ChidMatcher<'_, '_, '_> {
    /// Create a new CHID mapping matcher
    pub fn new<'mappings, 'mapping_str, 'srcs>(
        mappings: &'mappings [ChidMapping<'mapping_str>],
        srcs: &'srcs ChidSources,
    ) -> ChidMatcher<'mappings, 'mapping_str, 'srcs> {
        ChidMatcher {
            mappings,
            srcs,
            mappings_idx: 0,
            type_priority_idx: 0,
            current_chid: None,
        }
    }
}

impl<'mappings, 'mapping_str, 'srcs> core::iter::Iterator
    for ChidMatcher<'mappings, 'mapping_str, 'srcs>
{
    type Item = &'mappings ChidMapping<'mapping_str>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let Some(current_chid) = &self.current_chid else {
                // No more types to try
                if self.type_priority_idx >= CHID_TYPE_MATCHING_PRIORITY.len() {
                    return None;
                }

                // Compute CHID for the next type
                self.current_chid = compute_chid(
                    self.srcs,
                    CHID_TYPES[CHID_TYPE_MATCHING_PRIORITY[self.type_priority_idx]],
                );
                self.type_priority_idx += 1;

                continue;
            };

            while self.mappings_idx < self.mappings.len() {
                let mapping = &self.mappings[self.mappings_idx];
                self.mappings_idx += 1;

                if mapping.chid() == current_chid {
                    return Some(mapping);
                }
            }

            // Reset for next type
            self.mappings_idx = 0;
            self.current_chid = None;
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::guid_str;

    #[test]
    fn test_empty() {
        let mappings: [ChidMapping; 0] = [];
        let srcs = ChidSources::default();
        let mut matcher = ChidMatcher::new(&mappings, &srcs);
        assert!(matcher.next().is_none());
    }

    #[test]
    fn test_no_match() {
        let mappings = [ChidMapping::DeviceTree {
            chid: guid_str("00000000-0000-0000-0000-000000000000"),
            name: Some("Example Device"),
            compatible: Some("example,device"),
        }];
        let srcs: ChidSources = [
            Some("LENOVO".into()),                             // CHID_SMBIOS_MANUFACTURER
            Some("Miix 630".into()),                           // CHID_SMBIOS_FAMILY
            Some("81F1".into()),                               // CHID_SMBIOS_PRODUCT_NAME
            Some("LENOVO_MT_81F1_BU_idea_FM_Miix 630".into()), // CHID_SMBIOS_PRODUCT_SKU
            Some("LENOVO".into()),                             // CHID_SMBIOS_BASEBOARD_MANUFACTURER
            Some("LNVNB161216".into()),                        // CHID_SMBIOS_BASEBOARD_PRODUCT
            Some("LENOVO".into()),                             // CHID_SMBIOS_BIOS_VENDOR
            Some("8WCN25WW".into()),                           // CHID_SMBIOS_BIOS_VERSION
            None,                                              // CHID_SMBIOS_BIOS_MAJOR
            None,                                              // CHID_SMBIOS_BIOS_MINOR
            Some("32".into()),                                 // CHID_SMBIOS_ENCLOSURE_TYPE
            None,                                              // CHID_EDID_PANEL
        ];
        let mut matcher = ChidMatcher::new(&mappings, &srcs);
        assert!(matcher.next().is_none());
    }

    #[test]
    fn test_some_match() {
        let mappings = [
            ChidMapping::DeviceTree {
                chid: guid_str("08b75d1f-6643-52a1-9bdd-071052860b33"),
                name: Some("LENOVO Miix 630"),
                compatible: Some("lenovo,miix-630"),
            },
            ChidMapping::DeviceTree {
                chid: guid_str("00000000-0000-0000-0000-000000000000"),
                name: Some("Example Device"),
                compatible: Some("example,device"),
            },
            ChidMapping::DeviceTree {
                chid: guid_str("14f581d2-d059-5cb2-9f8b-56d8be7932c9"),
                name: Some("LENOVO Miix 630"),
                compatible: Some("lenovo,miix-630"),
            },
        ];
        let srcs: ChidSources = [
            Some("LENOVO".into()),                             // CHID_SMBIOS_MANUFACTURER
            Some("Miix 630".into()),                           // CHID_SMBIOS_FAMILY
            Some("81F1".into()),                               // CHID_SMBIOS_PRODUCT_NAME
            Some("LENOVO_MT_81F1_BU_idea_FM_Miix 630".into()), // CHID_SMBIOS_PRODUCT_SKU
            Some("LENOVO".into()),                             // CHID_SMBIOS_BASEBOARD_MANUFACTURER
            Some("LNVNB161216".into()),                        // CHID_SMBIOS_BASEBOARD_PRODUCT
            Some("LENOVO".into()),                             // CHID_SMBIOS_BIOS_VENDOR
            Some("8WCN25WW".into()),                           // CHID_SMBIOS_BIOS_VERSION
            None,                                              // CHID_SMBIOS_BIOS_MAJOR
            None,                                              // CHID_SMBIOS_BIOS_MINOR
            Some("32".into()),                                 // CHID_SMBIOS_ENCLOSURE_TYPE
            None,                                              // CHID_EDID_PANEL
        ];
        let mut matcher = ChidMatcher::new(&mappings, &srcs);
        assert!(matches!(
            matcher.next(),
            Some(ChidMapping::DeviceTree {
                compatible: Some("lenovo,miix-630"),
                ..
            })
        ));
        assert!(matches!(
            matcher.next(),
            Some(ChidMapping::DeviceTree {
                compatible: Some("lenovo,miix-630"),
                ..
            })
        ));
        assert!(matcher.next().is_none());
    }
}

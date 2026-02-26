//! Capability manifest: deny-by-default runtime permission model.

use serde::Deserialize;
use std::collections::HashMap;

/// A capability manifest governing what effects a program may perform at runtime.
///
/// When present, only capabilities explicitly listed in `caps` are allowed.
/// When absent (i.e. `Option<CapabilityManifest>` is `None`), all capabilities
/// are permitted (backward-compatible allow-all).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityManifest {
    #[serde(default)]
    pub caps: HashMap<String, CapabilityGrant>,
}

/// Fine-grained constraints on a granted capability.
///
/// An empty grant (all fields `None`) means the capability is granted
/// without restrictions.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CapabilityGrant {
    pub allow_domains: Option<Vec<String>>,
    pub allow_tables: Option<Vec<String>>,
    pub allow_keys: Option<Vec<String>>,
}

impl CapabilityManifest {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn is_granted(&self, cap_name: &str) -> bool {
        self.caps.contains_key(cap_name)
    }
}

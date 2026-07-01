//! Versioned native specialization adapter registry.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::AdapterRequirement;

/// Installed native specialization adapter identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeAdapter {
    /// Stable registry ID.
    pub id: String,
    /// Exact implementation version participating in cache keys.
    pub version: String,
}

/// Deterministically ordered native specialization adapter registry.
#[derive(Debug, Clone, Default)]
pub struct NativeAdapterRegistry {
    /// Installed adapters keyed by stable ID.
    adapters: BTreeMap<String, NativeAdapter>,
}

impl NativeAdapterRegistry {
    /// Creates an empty adapter registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            adapters: BTreeMap::new(),
        }
    }

    /// Installs or replaces one exact adapter identity.
    pub fn register(&mut self, adapter: NativeAdapter) -> Option<NativeAdapter> {
        self.adapters.insert(adapter.id.clone(), adapter)
    }

    /// Returns whether an exact adapter requirement is installed.
    #[must_use]
    pub fn supports(&self, requirement: &AdapterRequirement) -> bool {
        self.adapters
            .get(&requirement.id)
            .is_some_and(|adapter| adapter.version == requirement.version)
    }

    /// Returns requirements that cannot be satisfied by this registry.
    #[must_use]
    pub fn missing<'a>(
        &self,
        requirements: &'a [AdapterRequirement],
    ) -> Vec<&'a AdapterRequirement> {
        requirements
            .iter()
            .filter(|requirement| !self.supports(requirement))
            .collect()
    }

    /// Returns sorted adapter ID/version pairs for specialization cache keys.
    #[must_use]
    pub fn versions(&self) -> Vec<(String, String)> {
        self.adapters
            .values()
            .map(|adapter| (adapter.id.clone(), adapter.version.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_requires_exact_adapter_versions() {
        let mut registry = NativeAdapterRegistry::new();
        registry.register(NativeAdapter {
            id: "python.native-callable:pkg.fn".to_owned(),
            version: "2".to_owned(),
        });
        let requirement = AdapterRequirement {
            id: "python.native-callable:pkg.fn".to_owned(),
            version: "1".to_owned(),
            reason: "opaque callable".to_owned(),
            span: None,
        };
        assert!(
            !registry.supports(&requirement),
            "adapter versions must participate in compatibility and cache identity"
        );
    }
}

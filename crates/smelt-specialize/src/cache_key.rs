//! Deterministic specialization cache keys.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{HashInputs, HostLanguage, SandboxPolicyRecord};

/// Inputs whose exact bytes determine one host-runtime specialization result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheKeyInput {
    /// Smelt compiler version.
    pub smelt_version: String,
    /// Manifest schema version.
    pub schema_version: u32,
    /// Guest language.
    pub language: HostLanguage,
    /// Exact host runtime version.
    pub host_runtime_version: String,
    /// Hashes of sources, dependencies, environment, and provenance.
    pub hashes: HashInputs,
    /// Effective hard-sandbox policy.
    pub sandbox_policy: SandboxPolicyRecord,
    /// Versioned native adapter identities, sorted by adapter ID.
    pub native_adapter_versions: Vec<(String, String)>,
}

/// Hex-encoded SHA-256 key for one specialization process and manifest.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SpecializationCacheKey(String);

impl SpecializationCacheKey {
    /// Computes a stable key from canonical schema data.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if a future schema field cannot be
    /// represented by the canonical JSON serializer.
    pub fn compute(input: &CacheKeyInput) -> Result<Self, serde_json::Error> {
        let bytes = serde_json::to_vec(input)?;
        let digest = Sha256::digest(bytes);
        Ok(Self(format!("{digest:x}")))
    }

    /// Returns the lowercase hexadecimal digest.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn input() -> CacheKeyInput {
        CacheKeyInput {
            smelt_version: "0.1.0".to_owned(),
            schema_version: 1,
            language: HostLanguage::Python,
            host_runtime_version: "3.13.5".to_owned(),
            hashes: HashInputs {
                source: "source".to_owned(),
                dependencies: "deps".to_owned(),
                lockfile: "lock".to_owned(),
                environment: "env".to_owned(),
                policy: "policy".to_owned(),
                callable_provenance: "callable".to_owned(),
            },
            sandbox_policy: SandboxPolicyRecord {
                backend: "linux-bwrap".to_owned(),
                network: false,
                writable_roots: vec!["/scratch".to_owned()],
                environment: BTreeMap::new(),
                wall_time_ms: 10_000,
                cpu_time_ms: 5_000,
                memory_bytes: 256 * 1024 * 1024,
                process_limit: 1,
                output_bytes: 1024 * 1024,
                subprocesses: false,
                native_extensions: Vec::new(),
            },
            native_adapter_versions: Vec::new(),
        }
    }

    #[test]
    fn cache_key_is_deterministic_and_sensitive_to_inputs() -> Result<(), String> {
        let first = SpecializationCacheKey::compute(&input()).map_err(|error| error.to_string())?;
        let second =
            SpecializationCacheKey::compute(&input()).map_err(|error| error.to_string())?;
        if first != second {
            return Err("identical inputs produced different cache keys".to_owned());
        }

        let mut changed = input();
        changed.hashes.environment = "different".to_owned();
        let changed_key =
            SpecializationCacheKey::compute(&changed).map_err(|error| error.to_string())?;
        if first == changed_key {
            return Err("environment changes did not invalidate specialization".to_owned());
        }
        Ok(())
    }
}

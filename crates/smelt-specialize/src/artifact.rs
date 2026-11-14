//! Persistent specialization cache and package-artifact storage.
//!
//! A specialization artifact is the durable seam between host execution and
//! frontend lowering. Cache hits and packaged dependency artifacts use the
//! same validated, versioned representation.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{HostLanguage, SpecializationCacheKey, SpecializationManifest, validate_manifest};

/// One cache-addressed specialization manifest suitable for package transport.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpecializationArtifact {
    /// Cache key covering every input that produced the manifest.
    pub cache_key: SpecializationCacheKey,
    /// Materialized definition-time result.
    pub manifest: SpecializationManifest,
}

/// Filesystem-backed specialization artifact store.
#[derive(Debug, Clone)]
pub struct SpecializationArtifactStore {
    /// Root containing language-specific artifact directories.
    root: PathBuf,
}

impl SpecializationArtifactStore {
    /// Creates a store rooted at `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Returns the artifact path for one language and cache key.
    #[must_use]
    pub fn artifact_path(&self, language: HostLanguage, key: &SpecializationCacheKey) -> PathBuf {
        self.root
            .join(language_directory(language))
            .join(format!("{}.json", key.as_str()))
    }

    /// Loads and validates an artifact when it exists.
    ///
    /// # Errors
    ///
    /// Returns an error for unreadable, malformed, mismatched, or structurally
    /// invalid artifacts. A missing artifact is a normal cache miss.
    pub fn load(
        &self,
        language: HostLanguage,
        key: &SpecializationCacheKey,
    ) -> Result<Option<SpecializationManifest>, ArtifactError> {
        let path = self.artifact_path(language, key);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(ArtifactError::Io(error)),
        };
        let artifact: SpecializationArtifact = serde_json::from_slice(&bytes)?;
        if &artifact.cache_key != key {
            return Err(ArtifactError::CacheKeyMismatch {
                expected: key.as_str().to_owned(),
                actual: artifact.cache_key.as_str().to_owned(),
            });
        }
        if artifact.manifest.language != language {
            return Err(ArtifactError::LanguageMismatch {
                expected: language,
                actual: artifact.manifest.language,
            });
        }
        validate_manifest(&artifact.manifest).map_err(ArtifactError::InvalidManifest)?;
        Ok(Some(artifact.manifest))
    }

    /// Writes an artifact only when its serialized bytes changed.
    ///
    /// Returns `true` when the filesystem was updated and `false` when the
    /// existing artifact was byte-identical.
    ///
    /// # Errors
    ///
    /// Returns an error for serialization or filesystem failures.
    pub fn store(
        &self,
        key: &SpecializationCacheKey,
        manifest: &SpecializationManifest,
    ) -> Result<bool, ArtifactError> {
        validate_manifest(manifest).map_err(ArtifactError::InvalidManifest)?;
        let artifact = SpecializationArtifact {
            cache_key: key.clone(),
            manifest: manifest.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&artifact)?;
        let path = self.artifact_path(manifest.language, key);
        write_if_changed(&path, &bytes)
    }

    /// Whether `path` is an artifact file this store wrote: it lies at
    /// `<root>/<language>/<key>.json` and holds an artifact whose cache key is
    /// `<key>` (the layout and content [`Self::store`] writes).
    ///
    /// `smelt clean` asks this to tell the build's own artifacts from files
    /// someone put in the output directory.
    #[must_use]
    pub fn wrote(&self, path: &Path) -> bool {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return false;
        };
        let mut components = relative.components();
        let (Some(language), Some(file), None) =
            (components.next(), components.next(), components.next())
        else {
            return false;
        };
        let known_language = [HostLanguage::Python, HostLanguage::TypeScript]
            .into_iter()
            .any(|known| language.as_os_str() == language_directory(known));
        let Some(stem) = Path::new(file.as_os_str())
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".json"))
        else {
            return false;
        };
        known_language
            && fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<SpecializationArtifact>(&bytes).ok())
                .is_some_and(|artifact| artifact.cache_key.as_str() == stem)
    }

    /// Copies a cached artifact into a package artifact directory.
    ///
    /// The package layout is identical to the cache layout, so consumers can
    /// point a store directly at an unpacked dependency artifact.
    ///
    /// # Errors
    ///
    /// Returns an error when the cache entry is absent or cannot be validated
    /// or written.
    pub fn publish(
        &self,
        package_root: &Path,
        language: HostLanguage,
        key: &SpecializationCacheKey,
    ) -> Result<bool, ArtifactError> {
        let manifest = self
            .load(language, key)?
            .ok_or_else(|| ArtifactError::Missing(key.as_str().to_owned()))?;
        Self::new(package_root).store(key, &manifest)
    }
}

/// Specialization artifact storage failure.
#[derive(Debug)]
pub enum ArtifactError {
    /// Filesystem operation failed.
    Io(io::Error),
    /// Artifact JSON was malformed.
    Decode(serde_json::Error),
    /// Artifact filename key and embedded key differ.
    CacheKeyMismatch {
        /// Key requested by the caller.
        expected: String,
        /// Key embedded in the artifact.
        actual: String,
    },
    /// Artifact was stored under the wrong language.
    LanguageMismatch {
        /// Language requested by the caller.
        expected: HostLanguage,
        /// Language embedded in the manifest.
        actual: HostLanguage,
    },
    /// Manifest failed structural validation.
    InvalidManifest(Vec<crate::ManifestError>),
    /// A requested cache entry did not exist.
    Missing(String),
}

impl std::fmt::Display for ArtifactError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "specialization artifact I/O failed: {error}"),
            Self::Decode(error) => {
                write!(
                    formatter,
                    "specialization artifact JSON is invalid: {error}"
                )
            }
            Self::CacheKeyMismatch { expected, actual } => write!(
                formatter,
                "specialization artifact cache key mismatch: expected {expected}, found {actual}"
            ),
            Self::LanguageMismatch { expected, actual } => write!(
                formatter,
                "specialization artifact language mismatch: expected {}, found {}",
                language_name(*expected),
                language_name(*actual),
            ),
            Self::InvalidManifest(errors) => write!(
                formatter,
                "specialization artifact manifest is invalid: {}",
                errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
            Self::Missing(key) => write!(formatter, "specialization cache entry {key} is missing"),
        }
    }
}

impl std::error::Error for ArtifactError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Decode(error) => Some(error),
            Self::CacheKeyMismatch { .. }
            | Self::LanguageMismatch { .. }
            | Self::InvalidManifest(_)
            | Self::Missing(_) => None,
        }
    }
}

impl From<io::Error> for ArtifactError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ArtifactError {
    fn from(error: serde_json::Error) -> Self {
        Self::Decode(error)
    }
}

/// Stable directory spelling used by caches and package artifacts.
const fn language_directory(language: HostLanguage) -> &'static str {
    match language {
        HostLanguage::Python => "python",
        HostLanguage::TypeScript => "typescript",
    }
}

/// Human-readable host language spelling used by diagnostics.
const fn language_name(language: HostLanguage) -> &'static str {
    match language {
        HostLanguage::Python => "Python",
        HostLanguage::TypeScript => "TypeScript",
    }
}

/// Creates parent directories and preserves mtime for identical bytes.
fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<bool, ArtifactError> {
    if fs::read(path).is_ok_and(|current| current == bytes) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{HashInputs, MANIFEST_SCHEMA_VERSION, SandboxPolicyRecord, ValueGraph};

    use super::*;

    /// Boxed dynamic error type shared by the artifact tests.
    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn artifact_round_trip_preserves_cycles_and_identity() -> TestResult {
        let temp = tempfile::tempdir()?;
        let store = SpecializationArtifactStore::new(temp.path());
        let key = key()?;
        let manifest = manifest();
        if !store.store(&key, &manifest)? {
            return Err("first artifact write did not update the filesystem".into());
        }
        if store.store(&key, &manifest)? {
            return Err("identical artifact bytes were rewritten".into());
        }
        let loaded = store
            .load(HostLanguage::Python, &key)?
            .ok_or("artifact cache miss")?;
        if loaded != manifest {
            return Err("artifact round trip changed the manifest".into());
        }
        Ok(())
    }

    #[test]
    fn packaged_artifact_uses_the_cache_layout() -> TestResult {
        let cache = tempfile::tempdir()?;
        let package = tempfile::tempdir()?;
        let store = SpecializationArtifactStore::new(cache.path());
        let key = key()?;
        store.store(&key, &manifest())?;
        if !store.publish(package.path(), HostLanguage::Python, &key)? {
            return Err("package artifact was not written".into());
        }
        let packaged =
            SpecializationArtifactStore::new(package.path()).load(HostLanguage::Python, &key)?;
        if packaged.is_none() {
            return Err("package artifact could not be loaded as a cache entry".into());
        }
        Ok(())
    }

    fn key() -> Result<SpecializationCacheKey, serde_json::Error> {
        let project =
            crate::ProjectRoot::new(Path::new("/project")).map_err(serde::de::Error::custom)?;
        SpecializationCacheKey::compute(&crate::CacheKeyInput::new(
            HostLanguage::Python,
            "test-runtime".to_owned(),
            hashes(),
            &policy(),
            &project,
        ))
    }

    fn manifest() -> SpecializationManifest {
        SpecializationManifest {
            smelt_version: "test".to_owned(),
            schema_version: MANIFEST_SCHEMA_VERSION,
            language: HostLanguage::Python,
            host_runtime_version: "test-runtime".to_owned(),
            hashes: hashes(),
            sandbox_policy: policy(),
            modules: Vec::new(),
            values: ValueGraph { nodes: Vec::new() },
            effects: Vec::new(),
            required_adapters: Vec::new(),
        }
    }

    fn hashes() -> HashInputs {
        HashInputs {
            source: "source".to_owned(),
            dependencies: "dependencies".to_owned(),
            lockfile: "lockfile".to_owned(),
            environment: "environment".to_owned(),
            policy: "policy".to_owned(),
            callable_provenance: "provenance".to_owned(),
        }
    }

    fn policy() -> SandboxPolicyRecord {
        SandboxPolicyRecord {
            backend: "test".to_owned(),
            network: false,
            read_only_roots: Vec::new(),
            writable_roots: Vec::new(),
            environment: BTreeMap::new(),
            wall_time_ms: 1,
            cpu_time_ms: 1,
            memory_bytes: 1,
            process_limit: 1,
            output_bytes: 1,
            subprocesses: false,
            native_extensions: Vec::new(),
        }
    }
}
